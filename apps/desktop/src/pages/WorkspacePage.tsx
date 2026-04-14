import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeRaw from 'rehype-raw';
import {
  useWorkspaceStore,
  WorkspaceDocument,
  Template,
  DocumentSection,
} from '../stores/useWorkspaceStore';

// ── 유틸 ────────────────────────────────────────────────────────────────────

function formatDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString('ko-KR', {
    month: 'short', day: 'numeric', year: 'numeric',
  });
}

const DOC_TYPE_ICONS: Record<string, string> = {
  note: '📝', meeting: '🗓', decision: '⚖️', journal: '📖',
  retrospective: '🔄', todo: '✅', techspec: '🔧', library: '📚',
  study: '📖', article: '📰', devlog: '💻', weekly: '📅',
  history: '🏛️', other: '📄',
};

// list_templates 응답 타입
interface TemplateSummary {
  name: string;
  description: string;
  source: 'builtin' | 'custom';
}

// 변수 파싱 (프론트엔드 간이 버전)
function extractVarsFromBody(body: string): string[] {
  const matches = body.matchAll(/\{\{([^}#/\s][^}\s]*)\}\}/g);
  const seen = new Set<string>();
  const result: string[] = [];
  for (const m of matches) {
    const name = m[1].trim();
    if (!seen.has(name) && name !== 'else' && name !== 'this') {
      seen.add(name);
      result.push(name);
    }
  }
  return result;
}

// ── 새 문서 모달 ─────────────────────────────────────────────────────────────

interface TemplateDetail {
  name: string;
  frontmatter_fields: string[];
  body_variables: string[];
}

// 자동 주입 (서버가 채움, 폼에서 숨김)
const AUTO_INJECT_FIELDS = new Set(['created', 'updated', 'aliases']);

function NewDocModal({
  allTemplates, onClose, onCreated,
}: {
  allTemplates: TemplateSummary[];
  onClose: () => void;
  onCreated: (doc: WorkspaceDocument) => void;
}) {
  const { createDocument } = useWorkspaceStore();
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);
  const [templateDetail, setTemplateDetail] = useState<TemplateDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [frontmatterValues, setFrontmatterValues] = useState<Record<string, string>>({});
  const [variableValues, setVariableValues] = useState<Record<string, string>>({});
  const [creating, setCreating] = useState(false);

  const handleSelectTemplate = async (name: string | null) => {
    setSelectedTemplate(name);
    setFrontmatterValues({});
    setVariableValues({});
    if (!name) { setTemplateDetail(null); return; }
    setLoadingDetail(true);
    try {
      const detail = await invoke<TemplateDetail>('get_template', { name });
      setTemplateDetail(detail);
    } catch (e) {
      console.error(e);
      setTemplateDetail(null);
    } finally { setLoadingDetail(false); }
  };

  const handleCreate = async () => {
    const title = frontmatterValues['title']?.trim() || variableValues['title']?.trim() || '';
    if (!title) return;
    setCreating(true);
    try {
      if (selectedTemplate && templateDetail) {
        const doc = await invoke<WorkspaceDocument>('apply_template', {
          template: selectedTemplate,
          frontmatter: frontmatterValues,
          variables: variableValues,
        });
        onCreated(doc);
      } else {
        const doc = await createDocument(title, undefined);
        onCreated(doc);
      }
    } catch (e) { console.error(e); }
    finally { setCreating(false); }
  };

  const fmFields = (templateDetail?.frontmatter_fields ?? []).filter(f => !AUTO_INJECT_FIELDS.has(f));
  const bodyVars = templateDetail?.body_variables ?? [];
  const hasVars = fmFields.length > 0 || bodyVars.length > 0;
  const titleValue = frontmatterValues['title'] ?? variableValues['title'] ?? '';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-lg shadow-2xl max-h-[90vh] flex flex-col">
        <h2 className="text-white font-semibold text-lg mb-4">새 문서</h2>

        {/* 템플릿 선택 */}
        <label className="block text-gray-400 text-sm mb-2">템플릿</label>
        <div className="grid grid-cols-3 gap-2 mb-5">
          <button onClick={() => handleSelectTemplate(null)}
            className={`px-3 py-2 rounded-lg text-sm text-left border transition-colors ${selectedTemplate === null ? 'bg-indigo-600 border-indigo-500 text-white' : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-gray-600'}`}>
            📄 빈 문서
          </button>
          {allTemplates.map((t) => (
            <button key={`tmpl-${t.name}`} onClick={() => handleSelectTemplate(t.name)}
              className={`px-3 py-2 rounded-lg text-sm text-left border transition-colors ${selectedTemplate === t.name ? 'bg-indigo-600 border-indigo-500 text-white' : 'bg-gray-800 border-gray-700 text-gray-400 hover:border-gray-600'}`}>
              {DOC_TYPE_ICONS[t.name] ?? '📄'} {t.description || t.name}
            </button>
          ))}
        </div>

        {/* 빈 문서 - 제목만 */}
        {selectedTemplate === null && (
          <div className="mb-4">
            <label className="block text-gray-400 text-sm mb-1">제목</label>
            <input autoFocus type="text"
              value={variableValues['title'] ?? ''}
              onChange={(e) => setVariableValues(v => ({ ...v, title: e.target.value }))}
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
              placeholder="제목 없음"
              className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm" />
          </div>
        )}

        {/* 템플릿 변수 폼 */}
        {selectedTemplate && (
          <div className="flex-1 overflow-y-auto">
            {loadingDetail ? (
              <p className="text-gray-500 text-sm text-center py-4">변수 로딩 중...</p>
            ) : hasVars ? (
              <>
                {fmFields.length > 0 && (
                  <div className="mb-4">
                    <p className="text-xs text-gray-500 font-medium uppercase tracking-widest mb-2">Frontmatter</p>
                    <div className="flex flex-col gap-2">
                      {fmFields.map(field => (
                        <div key={field}>
                          <label className="block text-gray-400 text-xs mb-1">{field}</label>
                          <input type="text"
                            autoFocus={field === 'title'}
                            value={frontmatterValues[field] ?? ''}
                            onChange={(e) => setFrontmatterValues(v => ({ ...v, [field]: e.target.value }))}
                            placeholder={field}
                            className="w-full px-3 py-1.5 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-600 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm" />
                        </div>
                      ))}
                    </div>
                  </div>
                )}
                {bodyVars.length > 0 && (
                  <div className="mb-4">
                    <p className="text-xs text-gray-500 font-medium uppercase tracking-widest mb-2">본문 변수</p>
                    <div className="flex flex-col gap-3">
                      {bodyVars.map(field => (
                        <div key={field}>
                          <label className="block text-gray-400 text-xs mb-1">{field}</label>
                          <textarea
                            value={variableValues[field] ?? ''}
                            onChange={(e) => setVariableValues(v => ({ ...v, [field]: e.target.value }))}
                            placeholder={field}
                            rows={3}
                            className="w-full px-3 py-1.5 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-600 focus:outline-none focus:ring-2 focus:ring-indigo-500 text-sm resize-y" />
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </>
            ) : (
              <p className="text-gray-500 text-sm text-center py-4">이 템플릿은 추가 변수가 없습니다</p>
            )}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-3 border-t border-gray-800 mt-3">
          <button onClick={onClose} className="px-4 py-1.5 rounded-lg text-sm text-gray-400 hover:text-white border border-gray-700 hover:border-gray-600 transition-colors">취소</button>
          <button onClick={handleCreate} disabled={!titleValue || creating}
            className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-4 py-1.5 rounded-lg text-sm transition-colors">
            {creating ? '생성 중...' : '생성'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── 문서 편집 모달 ───────────────────────────────────────────────────────────

type EditMode = 'full' | 'section';

function EditDocModal({
  doc, onClose, onSaved,
}: { doc: WorkspaceDocument; onClose: () => void; onSaved: () => void }) {
  const { updateDocument, getSections, updateSection } = useWorkspaceStore();
  const [mode, setMode] = useState<EditMode>('full');
  const [title, setTitle] = useState(doc.title);
  const [content, setContent] = useState('');
  const [sections, setSections] = useState<DocumentSection[]>([]);
  const [selectedHeading, setSelectedHeading] = useState('');
  const [sectionContent, setSectionContent] = useState('');
  const [saving, setSaving] = useState(false);

  // 모달 열릴 때 전체 content 로드 (content_preview는 100자만이라 사용 X)
  useEffect(() => {
    invoke<{ content: string }>('get_workspace_document', { docId: doc.id })
      .then((res) => setContent(res.content ?? ''))
      .catch((e) => console.error('[load doc content]', e));
  }, [doc.id]);

  useEffect(() => {
    if (mode === 'section') {
      getSections(doc.id).then((s) => {
        setSections(s);
        if (s.length > 0) {
          setSelectedHeading(s[0].heading);
          setSectionContent(s[0].content);
        }
      });
    }
  }, [mode, doc.id, getSections]);

  const handleSectionSelect = (heading: string) => {
    const sec = sections.find((s) => s.heading === heading);
    setSelectedHeading(heading);
    setSectionContent(sec?.content ?? '');
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      if (mode === 'full') {
        await updateDocument(doc.id, title, content);
      } else {
        await updateSection(doc.id, selectedHeading, sectionContent);
      }
      onSaved();
    } catch (e) { console.error(e); }
    finally { setSaving(false); }
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl p-6 w-full max-w-2xl flex flex-col gap-4 shadow-2xl">
        <div className="flex items-center justify-between">
          <input value={title} onChange={(e) => setTitle(e.target.value)}
            className="text-base font-semibold text-gray-100 bg-transparent border-b border-gray-700 focus:outline-none focus:border-indigo-500 flex-1" />
          <button onClick={onClose} className="text-gray-500 hover:text-gray-300 ml-4">✕</button>
        </div>

        <div className="flex gap-1 p-1 bg-gray-800 rounded-lg w-fit">
          {(['full', 'section'] as EditMode[]).map((m) => (
            <button key={m} onClick={() => setMode(m)}
              className={`px-3 py-1 rounded text-xs font-medium transition-colors ${mode === m ? 'bg-indigo-600 text-white' : 'text-gray-400 hover:text-white'}`}>
              {m === 'full' ? '전체 편집' : '섹션 편집'}
            </button>
          ))}
        </div>

        {mode === 'full' ? (
          <textarea value={content} onChange={(e) => setContent(e.target.value)}
            className="bg-gray-800 border border-gray-700 rounded-lg px-4 py-3 text-sm text-gray-100 placeholder-gray-600 focus:outline-none focus:border-indigo-500 resize-none h-64 font-mono" />
        ) : (
          <div className="flex flex-col gap-3">
            <select value={selectedHeading} onChange={(e) => handleSectionSelect(e.target.value)}
              className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
              {sections.length === 0 && <option>섹션 없음</option>}
              {sections.map((s) => (
                <option key={s.heading} value={s.heading}>{s.heading}</option>
              ))}
            </select>
            <textarea value={sectionContent} onChange={(e) => setSectionContent(e.target.value)}
              className="bg-gray-800 border border-gray-700 rounded-lg px-4 py-3 text-sm text-gray-100 focus:outline-none focus:border-indigo-500 resize-none h-48 font-mono" />
          </div>
        )}

        <div className="flex gap-2 justify-end">
          <button onClick={onClose} className="px-3 py-1.5 text-sm text-gray-400 hover:text-gray-200">취소</button>
          <button onClick={handleSave} disabled={saving}
            className="px-4 py-1.5 text-sm bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white rounded-lg transition-colors">
            {saving ? '저장 중...' : '저장'}
          </button>
        </div>
      </div>
    </div>
  );
}

// ── 템플릿 편집 모달 ─────────────────────────────────────────────────────────

// ── frontmatter 파서 (프론트엔드용) ──────────────────────────────────────────

function parseFrontmatter(content: string): { fields: [string, string][]; body: string } {
  const lines = content.split('\n');
  if (lines[0]?.trim() !== '---') return { fields: [], body: content };
  let endIdx = 0;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') { endIdx = i; break; }
  }
  if (endIdx === 0) return { fields: [], body: content };
  const fields: [string, string][] = [];
  for (let i = 1; i < endIdx; i++) {
    const colon = lines[i].indexOf(':');
    if (colon > 0) {
      fields.push([lines[i].slice(0, colon).trim(), lines[i].slice(colon + 1).trim()]);
    }
  }
  const bodyLines = lines.slice(endIdx + 1);
  const bodyStart = bodyLines.findIndex((l) => l.trim() !== '');
  const body = bodyLines.slice(Math.max(bodyStart, 0)).join('\n');
  return { fields, body };
}

function buildContent(fields: [string, string][], body: string): string {
  if (fields.length === 0) return body;
  return `---\n${fields.map(([k, v]) => `${k}: ${v}`).join('\n')}\n---\n\n${body}`;
}

// ── 템플릿 편집 모달 ─────────────────────────────────────────────────────────

type TemplateTab = 'frontmatter' | 'body' | 'variables' | 'raw';

function TemplateModal({
  existing, onClose, onSaved,
}: { existing?: Template; onClose: () => void; onSaved: () => void }) {
  const { createTemplate, updateTemplate } = useWorkspaceStore();
  const REQUIRED_FIELDS: [string, string][] = [
    ['title', ''], ['aliases', '[]'], ['created', ''], ['updated', ''], ['tags', '[]'],
  ];
  const initialContent = existing?.content ?? '';
  const parsed = parseFrontmatter(initialContent);
  // 새 템플릿이면 필수 필드 자동 추가
  const initialFields = parsed.fields.length > 0
    ? parsed.fields
    : REQUIRED_FIELDS;

  const [tab, setTab] = useState<TemplateTab>('frontmatter');
  const [name, setName] = useState(existing?.name ?? '');
  const [docType, setDocType] = useState(existing?.doc_type ?? 'note');
  const [description, setDescription] = useState(existing?.description ?? '');
  const [fields, setFields] = useState<[string, string][]>(initialFields);
  const [body, setBody] = useState(parsed.body);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [newVarName, setNewVarName] = useState('');
  const [editingVar, setEditingVar] = useState<{ original: string; value: string } | null>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  const insertAtCursor = (varName: string) => {
    const ta = bodyRef.current;
    if (!ta) return;
    const start = ta.selectionStart ?? body.length;
    const end = ta.selectionEnd ?? start;
    const newContent = body.slice(0, start) + `{{${varName}}}` + body.slice(end);
    setBody(newContent);
    setTimeout(() => {
      ta.selectionStart = ta.selectionEnd = start + varName.length + 4;
      ta.focus();
    }, 0);
  };

  const addVarToBody = (varName: string) => {
    const trimmed = varName.trim();
    if (!trimmed) return;
    setBody(prev => prev + `\n{{${trimmed}}}`);
    setNewVarName('');
  };

  const renameVar = (original: string, next: string) => {
    const trimmed = next.trim();
    if (!trimmed || trimmed === original) { setEditingVar(null); return; }
    const regex = new RegExp(`\\{\\{${original}\\}\\}`, 'g');
    setBody(prev => prev.replace(regex, `{{${trimmed}}}`));
    setEditingVar(null);
  };

  const deleteVar = (varName: string) => {
    const regex = new RegExp(`\\{\\{${varName}\\}\\}`, 'g');
    setBody(prev => prev.replace(regex, ''));
  };

  const updateField = (idx: number, col: 0 | 1, value: string) => {
    const next = [...fields];
    next[idx] = [...next[idx]] as [string, string];
    next[idx][col] = value;
    setFields(next);
  };
  const addField = () => setFields([...fields, ['', '']]);
  const removeField = (idx: number) => setFields(fields.filter((_, i) => i !== idx));

  const handleSave = async () => {
    if (!name.trim()) return;
    setSaving(true);
    setSaveError(null);
    const content = buildContent(fields.filter(([k]) => k.trim()), body);
    try {
      if (existing && existing.id > 0) {
        await updateTemplate(existing.id, name.trim(), docType, content, description || undefined);
      } else {
        await createTemplate(name.trim(), docType, content, description || undefined);
      }
      onSaved();
    } catch (e) {
      console.error('[template save error]', e);
      setSaveError(String(e));
    } finally { setSaving(false); }
  };

  const modalTitle = existing && existing.id > 0
    ? '템플릿 수정'
    : existing
    ? '기본 템플릿 커스터마이즈'
    : '새 템플릿';

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-gray-900 border border-gray-800 rounded-2xl w-full max-w-2xl shadow-2xl flex flex-col" style={{ maxHeight: '85vh' }}>
        {/* 헤더 */}
        <div className="flex items-center justify-between px-6 pt-5 pb-3 border-b border-gray-800">
          <div className="flex items-center gap-3">
            <span className="text-2xl">{DOC_TYPE_ICONS[docType] ?? '📄'}</span>
            <div>
              <h2 className="text-white font-semibold">{modalTitle}</h2>
              {description && <p className="text-gray-400 text-xs mt-0.5">{description}</p>}
            </div>
          </div>
          <button onClick={onClose} className="text-gray-500 hover:text-gray-300 text-lg leading-none">✕</button>
        </div>

        {/* 기본 정보 */}
        <div className="flex flex-col gap-2 px-6 pt-4 pb-2">
          <div className="flex gap-3">
            <div className="flex-1">
              <label className="block text-gray-400 text-xs mb-1">이름</label>
              <input value={name} onChange={(e) => setName(e.target.value)}
                className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500" />
            </div>
            <div>
              <label className="block text-gray-400 text-xs mb-1">유형</label>
              <select value={docType} onChange={(e) => setDocType(e.target.value)}
                className="px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500">
                {Object.keys(DOC_TYPE_ICONS).map((k) => <option key={k} value={k}>{k}</option>)}
              </select>
            </div>
          </div>
          <div>
            <label className="block text-gray-400 text-xs mb-1">설명</label>
            <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="템플릿에 대한 간단한 설명"
              className="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-2 focus:ring-indigo-500" />
          </div>
        </div>

        {/* 탭 */}
        <div className="flex gap-1 p-1 mx-6 mt-2 bg-gray-800 rounded-lg w-fit">
          {([['frontmatter', 'Frontmatter'], ['variables', '변수'], ['body', '본문'], ['raw', '전체 미리보기']] as [TemplateTab, string][]).map(([t, label]) => (
            <button key={t} onClick={() => setTab(t)}
              className={`px-3 py-1 rounded text-xs font-medium transition-colors ${tab === t ? 'bg-indigo-600 text-white' : 'text-gray-400 hover:text-white'}`}>
              {label}
            </button>
          ))}
        </div>

        {/* 본문 */}
        <div className="flex-1 overflow-auto px-6 py-4">
          {tab === 'frontmatter' && (() => {
            const REQUIRED_KEYS = ['title', 'aliases', 'created', 'updated', 'tags'];
            const isRequired = (key: string) => REQUIRED_KEYS.includes(key);
            return (
              <div className="flex flex-col gap-2">
                <p className="text-xs text-gray-500 mb-1">
                  <span className="text-amber-400">*</span> 필수 필드: title, aliases, created, updated, tags
                </p>
                {/* 필드 헤더 */}
                {fields.length > 0 && (
                  <div className="grid grid-cols-[1fr_2fr_auto] gap-2 text-xs text-gray-500 px-1">
                    <span>키</span><span>값</span><span className="w-7" />
                  </div>
                )}
                {/* 필드 목록 */}
                {fields.map(([key, val], i) => (
                  <div key={i} className="grid grid-cols-[1fr_2fr_auto] gap-2 items-center">
                    <div className="relative">
                      <input value={key} onChange={(e) => updateField(i, 0, e.target.value)}
                        placeholder="key"
                        className={`w-full px-2 py-1.5 bg-gray-800 border rounded-lg text-white text-sm font-mono focus:outline-none focus:ring-1 focus:ring-indigo-500 ${isRequired(key) ? 'border-amber-600/40' : 'border-gray-700'}`} />
                      {isRequired(key) && <span className="absolute -top-1 -right-1 text-amber-400 text-xs">*</span>}
                    </div>
                    <input value={val} onChange={(e) => updateField(i, 1, e.target.value)}
                      placeholder={isRequired(key) ? '필수' : 'value'}
                      className="px-2 py-1.5 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500" />
                    {isRequired(key) ? (
                      <span className="w-7 h-7 flex items-center justify-center text-gray-700 text-xs" title="필수 필드">🔒</span>
                    ) : (
                      <button onClick={() => removeField(i)}
                        className="text-gray-600 hover:text-red-400 text-sm w-7 h-7 flex items-center justify-center rounded hover:bg-gray-800 transition-colors">
                        ✕
                      </button>
                    )}
                  </div>
                ))}
                {/* 필드 추가 */}
                <button onClick={addField}
                  className="text-indigo-400 hover:text-indigo-300 text-xs mt-1 w-fit flex items-center gap-1">
                  + 필드 추가
                </button>
                {fields.length === 0 && (
                  <p className="text-gray-600 text-sm text-center py-6">frontmatter 필드가 없습니다. 위 버튼으로 추가하세요.</p>
                )}
              </div>
            );
          })()}

          {tab === 'body' && (
            <textarea ref={bodyRef} value={body} onChange={(e) => setBody(e.target.value)}
              placeholder={'# 제목\n\n## 섹션1\n\n내용을 입력하세요.'}
              className="w-full bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-100 focus:outline-none focus:border-indigo-500 resize-none h-56 font-mono" />
          )}

          {tab === 'variables' && (() => {
            const vars = extractVarsFromBody(body);
            return (
              <div className="flex flex-col gap-4">
                <div>
                  <p className="text-xs text-gray-500 mb-2">현재 본문의 변수 목록</p>
                  {vars.length === 0 ? (
                    <p className="text-gray-600 text-sm text-center py-4">본문에 {'{{변수}}'} 형식의 변수가 없습니다</p>
                  ) : (
                    <div className="flex flex-col gap-1">
                      {vars.map((v) => (
                        <div key={v} className="flex items-center gap-2 bg-gray-800 rounded-lg px-3 py-2">
                          {editingVar?.original === v ? (
                            <input
                              autoFocus
                              value={editingVar.value}
                              onChange={(e) => setEditingVar({ original: v, value: e.target.value })}
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') renameVar(v, editingVar.value);
                                if (e.key === 'Escape') setEditingVar(null);
                              }}
                              onBlur={() => renameVar(v, editingVar.value)}
                              className="flex-1 bg-gray-700 border border-indigo-500 rounded px-2 py-0.5 text-white text-sm font-mono focus:outline-none"
                            />
                          ) : (
                            <span
                              className="flex-1 text-indigo-300 font-mono text-sm cursor-pointer hover:text-indigo-200"
                              onClick={() => setEditingVar({ original: v, value: v })}
                              title="클릭하여 이름 변경"
                            >{`{{${v}}}`}</span>
                          )}
                          <button
                            onClick={() => setEditingVar({ original: v, value: v })}
                            className="text-gray-500 hover:text-gray-300 text-xs px-1.5 py-0.5 rounded border border-gray-700 hover:border-gray-500 transition-colors">
                            수정
                          </button>
                          <button
                            onClick={() => deleteVar(v)}
                            className="text-gray-500 hover:text-red-400 text-xs px-1.5 py-0.5 rounded border border-gray-700 hover:border-red-500 transition-colors">
                            삭제
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
                <div>
                  <p className="text-xs text-gray-500 mb-2">새 변수 추가</p>
                  <div className="flex gap-2">
                    <input
                      value={newVarName}
                      onChange={(e) => setNewVarName(e.target.value)}
                      onKeyDown={(e) => e.key === 'Enter' && addVarToBody(newVarName)}
                      placeholder="변수명"
                      className="flex-1 px-3 py-1.5 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm focus:outline-none focus:ring-1 focus:ring-indigo-500 font-mono" />
                    <button onClick={() => addVarToBody(newVarName)}
                      disabled={!newVarName.trim()}
                      className="px-3 py-1.5 bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white text-sm rounded-lg transition-colors">
                      추가 + 본문 삽입
                    </button>
                  </div>
                </div>
              </div>
            );
          })()}

          {tab === 'raw' && (() => {
            const fullContent = buildContent(fields.filter(([k]) => k.trim()), body);
            if (!fullContent) return <p className="text-gray-600 text-sm text-center py-8">(빈 템플릿)</p>;
            const fmFields = fields.filter(([k]) => k.trim());
            return (
              <div className="flex flex-col gap-4">
                {/* frontmatter 테이블 */}
                {fmFields.length > 0 && (
                  <div className="bg-gray-800 rounded-xl px-4 py-3 border border-gray-700">
                    <p className="text-xs text-gray-500 font-medium uppercase tracking-widest mb-2">Frontmatter</p>
                    <table className="w-full text-sm">
                      <tbody>
                        {fmFields.map(([k, v], i) => (
                          <tr key={i} className="border-b border-gray-700/50 last:border-0">
                            <td className="py-1 pr-4 text-gray-400 font-mono text-xs whitespace-nowrap">{k}</td>
                            <td className="py-1 text-gray-200 text-xs">{v || <span className="text-gray-600 italic">비어있음</span>}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
                {/* 마크다운 렌더링 */}
                <div className="bg-gray-800 rounded-xl px-5 py-4 prose prose-invert prose-sm max-w-none
                  prose-headings:text-gray-100 prose-p:text-gray-300 prose-li:text-gray-300
                  prose-strong:text-white prose-a:text-indigo-400
                  prose-code:text-indigo-300 prose-code:bg-gray-700 prose-code:px-1 prose-code:rounded
                  prose-hr:border-gray-700 prose-th:text-gray-300
                  prose-ul:marker:text-gray-500 prose-ol:marker:text-gray-500">
                  <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeRaw]}>
                    {body}
                  </ReactMarkdown>
                </div>
              </div>
            );
          })()}
        </div>

        {/* 푸터 */}
        <div className="flex flex-col gap-2 px-6 py-4 border-t border-gray-800">
          {saveError && (
            <p className="text-red-400 text-xs text-right">{saveError}</p>
          )}
          <div className="flex justify-end items-center gap-2">
            <button onClick={onClose} className="px-4 py-1.5 text-sm text-gray-400 hover:text-white border border-gray-700 rounded-lg">닫기</button>
            <button onClick={handleSave} disabled={!name.trim() || saving}
              className="bg-indigo-600 hover:bg-indigo-700 disabled:opacity-50 text-white px-4 py-1.5 rounded-lg text-sm transition-colors">
              {saving ? '저장 중...' : existing && existing.id > 0 ? '수정 저장' : '템플릿 저장'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ── 메인 페이지 ──────────────────────────────────────────────────────────────

type TabKey = 'documents' | 'templates';

export default function WorkspacePage() {
  const {
    workspace, documents, templates, isLoading,
    initWorkspace, fetchDocuments, fetchTemplates,
    deleteDocument, deleteTemplate,
  } = useWorkspaceStore();

  const [activeTab, setActiveTab] = useState<TabKey>('documents');
  const [showDocModal, setShowDocModal] = useState(false);
  const [showTmplModal, setShowTmplModal] = useState(false);
  const [editingDoc, setEditingDoc] = useState<WorkspaceDocument | null>(null);
  const [editingTemplate, setEditingTemplate] = useState<Template | undefined>(undefined);
  const [expandedDocId, setExpandedDocId] = useState<number | null>(null);
  const [allTemplates, setAllTemplates] = useState<TemplateSummary[]>([]);

  useEffect(() => {
    initWorkspace();
    invoke<{ templates: TemplateSummary[] }>('list_templates', {}).then(r => setAllTemplates(r.templates)).catch(console.error);
    fetchDocuments();
    fetchTemplates();
  }, [initWorkspace, fetchDocuments, fetchTemplates]);

  const tabs: { key: TabKey; label: string }[] = [
    { key: 'documents', label: '문서' },
    { key: 'templates', label: '템플릿' },
  ];

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6 gap-5">
      {/* 헤더 */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-white text-xl font-semibold tracking-tight">워크스페이스</h1>
          <p className="text-gray-400 text-sm mt-0.5">
            {workspace ? workspace.display_name : '개인 문서 및 템플릿 관리'}
          </p>
        </div>
        <div className="flex gap-2">
          {activeTab === 'documents' && (
            <button onClick={() => setShowDocModal(true)}
              className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors">
              + 새 문서
            </button>
          )}
          {activeTab === 'templates' && (
            <button onClick={() => { setEditingTemplate(undefined); setShowTmplModal(true); }}
              className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors">
              + 새 템플릿
            </button>
          )}
        </div>
      </div>

      {/* 탭 */}
      <div className="flex gap-1 p-1 bg-gray-900 border border-gray-800 rounded-xl w-fit">
        {tabs.map((tab) => (
          <button key={tab.key} onClick={() => setActiveTab(tab.key)}
            className={`px-4 py-1.5 rounded-lg text-sm font-medium transition-colors ${activeTab === tab.key ? 'bg-indigo-600 text-white' : 'text-gray-400 hover:text-white'}`}>
            {tab.label}
          </button>
        ))}
      </div>

      {/* 문서 탭 */}
      {activeTab === 'documents' && (
        <div className="flex-1 overflow-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-32">
              <p className="text-gray-500 text-sm">불러오는 중...</p>
            </div>
          ) : documents.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 gap-3">
              <p className="text-gray-500 text-sm">문서가 없습니다.</p>
              <button onClick={() => setShowDocModal(true)}
                className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors">
                첫 번째 문서 만들기
              </button>
            </div>
          ) : (
            <div className="grid gap-3">
              {documents.map((doc) => (
                <div key={doc.id} className="bg-gray-900 border border-gray-800 rounded-xl p-4 hover:border-gray-700 transition-colors">
                  <div className="flex items-start justify-between gap-3">
                    <button className="flex-1 min-w-0 text-left" onClick={() => setExpandedDocId(expandedDocId === doc.id ? null : doc.id)}>
                      <div className="flex items-center gap-2">
                        <span>{DOC_TYPE_ICONS[doc.doc_type ?? 'note'] ?? '📄'}</span>
                        <h3 className="text-white font-semibold truncate">{doc.title}</h3>
                      </div>
                      <p className="text-gray-400 text-sm mt-0.5">{formatDate(doc.created_at)}</p>
                    </button>
                    <div className="flex items-center gap-2 shrink-0">
                      <button onClick={() => setEditingDoc(doc)}
                        className="text-gray-500 hover:text-indigo-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-indigo-500 transition-colors">
                        수정
                      </button>
                      <button onClick={() => deleteDocument(doc.id)}
                        className="text-gray-500 hover:text-red-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-red-500 transition-colors">
                        삭제
                      </button>
                      <span className="text-gray-600 text-xs">{expandedDocId === doc.id ? '▲' : '▼'}</span>
                    </div>
                  </div>
                  {expandedDocId === doc.id && (
                    <div className="mt-3 pt-3 border-t border-gray-800">
                      {doc.content_preview ? (
                        <p className="text-gray-400 text-sm leading-relaxed">{doc.content_preview}</p>
                      ) : (
                        <p className="text-gray-600 text-sm italic">빈 문서입니다</p>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 템플릿 탭 */}
      {activeTab === 'templates' && (
        <div className="flex-1 overflow-auto flex flex-col gap-5">
          {/* 기본 제공 템플릿 */}
          <div>
            <h2 className="text-gray-500 text-xs font-medium uppercase tracking-widest mb-3">기본 제공</h2>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {allTemplates.filter(t => t.source === 'builtin').map((t) => {
                const override = templates.find(c => c.doc_type === t.name);
                return (
                  <button key={t.name}
                    onClick={async () => {
                      if (override) {
                        setEditingTemplate(override);
                      } else {
                        const detail = await invoke<{ content: string }>('get_template', { name: t.name }).catch(() => null);
                        setEditingTemplate({ id: 0, name: t.name, doc_type: t.name, content: detail?.content ?? '', created_at: 0, description: t.description });
                      }
                      setShowTmplModal(true);
                    }}
                    className={`bg-gray-900 border ${override ? 'border-indigo-800' : 'border-gray-800'} hover:border-gray-600 rounded-xl p-4 flex items-center gap-3 text-left transition-colors w-full relative`}>
                    {override && <span className="absolute top-2 right-2 text-[10px] text-indigo-400 bg-indigo-900/50 px-1.5 py-0.5 rounded">수정됨</span>}
                    <span className="text-2xl">{DOC_TYPE_ICONS[t.name] ?? '📄'}</span>
                    <div>
                      <h3 className="text-white font-semibold text-sm">{override ? override.name : t.name}</h3>
                      <p className="text-gray-500 text-xs">{override?.description || t.description || '클릭하여 커스터마이즈'}</p>
                    </div>
                  </button>
                );
              })}
            </div>
          </div>

          {/* 저장된 템플릿 — 빌트인 오버라이드는 제외 (기본 제공 섹션에서 표시) */}
          {(() => {
            const builtinNames = new Set(allTemplates.filter(t => t.source === 'builtin').map(t => t.name));
            const customOnly = templates.filter(t => !builtinNames.has(t.doc_type));
            return (
          <div>
            <h2 className="text-gray-500 text-xs font-medium uppercase tracking-widest mb-3">저장된 템플릿</h2>
            {customOnly.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-32 gap-3 border border-dashed border-gray-800 rounded-xl">
                <p className="text-gray-500 text-sm">저장된 템플릿이 없습니다.</p>
                <button onClick={() => { setEditingTemplate(undefined); setShowTmplModal(true); }}
                  className="text-indigo-400 hover:text-indigo-300 text-sm underline">
                  새 템플릿 만들기
                </button>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {customOnly.map((t) => (
                  <div key={t.id}
                    className="bg-gray-900 border border-gray-800 hover:border-gray-600 rounded-xl p-4 flex flex-col gap-3 cursor-pointer transition-colors"
                    onClick={() => { setEditingTemplate(t); setShowTmplModal(true); }}>
                    <div className="flex items-center gap-2">
                      <span className="text-2xl">{DOC_TYPE_ICONS[t.doc_type] ?? '📄'}</span>
                      <div>
                        <h3 className="text-white font-semibold">{t.name}</h3>
                        {t.project_id && <span className="text-xs text-indigo-400">프로젝트 전용</span>}
                      </div>
                    </div>
                    {t.description && <p className="text-gray-400 text-sm flex-1">{t.description}</p>}
                    <div className="flex gap-2" onClick={(e) => e.stopPropagation()}>
                      <button onClick={() => setShowDocModal(true)}
                        className="bg-indigo-600 hover:bg-indigo-700 text-white px-3 py-1.5 rounded-lg text-sm transition-colors flex-1">
                        사용하기
                      </button>
                      <button onClick={() => { setEditingTemplate(t); setShowTmplModal(true); }}
                        className="text-gray-400 hover:text-white text-xs px-2 py-1 rounded border border-gray-700 hover:border-gray-500 transition-colors">
                        수정
                      </button>
                      <button onClick={() => deleteTemplate(t.id)}
                        className="text-gray-500 hover:text-red-400 text-xs px-2 py-1 rounded border border-gray-700 hover:border-red-500 transition-colors">
                        삭제
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
            );
          })()}
        </div>
      )}

      {/* 모달들 */}
      {showDocModal && (
        <NewDocModal allTemplates={allTemplates}
          onClose={() => setShowDocModal(false)}
          onCreated={() => { setShowDocModal(false); fetchDocuments(); }} />
      )}
      {showTmplModal && (
        <TemplateModal existing={editingTemplate}
          onClose={() => setShowTmplModal(false)}
          onSaved={() => {
            setShowTmplModal(false);
            fetchTemplates();
            invoke<{ templates: TemplateSummary[] }>('list_templates', {}).then(r => setAllTemplates(r.templates)).catch(console.error);
          }} />
      )}
      {editingDoc && (
        <EditDocModal doc={editingDoc}
          onClose={() => setEditingDoc(null)}
          onSaved={() => { setEditingDoc(null); fetchDocuments(); }} />
      )}
    </div>
  );
}
