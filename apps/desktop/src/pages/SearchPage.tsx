import { useEffect, useRef, useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useSearchStore } from '../stores/useSearchStore';
import { usePluginStore } from '../stores/usePluginStore';

import { SearchHeader } from '../components/search/SearchHeader';
import { SearchSidebar } from '../components/search/SearchSidebar';
import { SearchPreview } from '../components/search/SearchPreview';
import { AdvancedSearchPanel } from '../components/search/AdvancedSearchPanel';
import { DocEntry } from '../components/search/SearchTree';
import { stripFrontmatter, hitToEntry, allDocToEntry } from '../utils/searchUtils';

export function SearchPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const {
    query, filters, hits, isLoading, error,
    setQuery, setFilters, search, clear,
    documentsById, allDocsLoading, listAllDocuments,
    updateDocumentMetadata
  } = useSearchStore();

  const allDocuments = useMemo(() => Object.values(documentsById), [documentsById]);

  const getEmoji = usePluginStore((s) => s.getEmoji);

  const [inputValue, setInputValue] = useState(query);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [selectedDoc, setSelectedDoc] = useState<DocEntry | null>(null);
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [previewMeta, setPreviewMeta] = useState<any | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const processedDocIdRef = useRef<string | null>(null);

  // Listeners for background updates
  useEffect(() => {
    const unlistenDoc = listen<{ source_doc_id: string; last_indexed: number }>('document-indexed', (e) => {
      updateDocumentMetadata(e.payload.source_doc_id, { last_indexed: e.payload.last_indexed });
    });

    let refreshTimer: ReturnType<typeof setTimeout> | null = null;
    const unlistenProj = listen<{ indexed: number; project_name: string }>('project-indexed', (e) => {
      if (e.payload.indexed > 0) {
        if (refreshTimer) clearTimeout(refreshTimer);
        refreshTimer = setTimeout(() => {
          listAllDocuments();
          // 검색어가 있는 경우 hits도 갱신
          const { query: q, filters: f, search: runSearch } = useSearchStore.getState();
          if (q.trim() || f.tagQuery.trim()) {
            runSearch();
          }
        }, 300);
      }
    });

    return () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      unlistenDoc.then(f => f());
      unlistenProj.then(f => f());
    };
  }, [updateDocumentMetadata, listAllDocuments]);

  // Load documents on mount (Limited to 1,000 in store)
  useEffect(() => { listAllDocuments(); }, [listAllDocuments]);

  // Sync URL state (Read)
  useEffect(() => {
    const docId = searchParams.get('docId');
    const tag = searchParams.get('tag');
    const project = searchParams.get('project');
    const q = searchParams.get('q');

    let shouldSearch = false;

    if (tag && tag !== filters.tagQuery) {
      setFilters({ tagQuery: tag });
      shouldSearch = true;
    }
    if (project) {
      const pList = project.split(',').filter(Boolean);
      if (JSON.stringify(pList) !== JSON.stringify(filters.projectNames)) {
        setFilters({ projectNames: pList });
        shouldSearch = true;
      }
    }
    if (q && q !== query) {
      setQuery(q);
      setInputValue(q);
      shouldSearch = true;
    }

    if (docId && docId !== processedDocIdRef.current) {
      const numericId = parseInt(docId, 10);
      processedDocIdRef.current = docId;

      invoke<any>('get_document_content', { documentId: numericId })
        .then(doc => {
          if (doc && selectedDoc?.document_id !== numericId) {
            const entry = {
              document_id: doc.document_id,
              title: doc.title || 'Untitled',
              file_path: doc.file_path,
              content_hash: doc.content_hash,
              source_doc_id: doc.source_doc_id,
              source_type: doc.source_type,
              last_indexed: doc.last_indexed,
            } as any;
            handleSelectDoc(entry);
          }
        })
        .catch(console.error);
    }

    if (shouldSearch) {
      search();
    }
  }, [searchParams, selectedDoc]);

  // Sync URL state (Write)
  useEffect(() => {
    const params = new URLSearchParams(searchParams);
    let changed = false;

    const currentTag = params.get('tag') || '';
    if (filters.tagQuery !== currentTag) {
      if (filters.tagQuery) params.set('tag', filters.tagQuery.replace(/^#/, ''));
      else params.delete('tag');
      changed = true;
    }

    const currentProject = params.get('project') || '';
    const newProject = filters.projectNames.join(',');
    if (newProject !== currentProject) {
      if (newProject) params.set('project', newProject);
      else params.delete('project');
      changed = true;
    }

    const currentQ = params.get('q') || '';
    if (query !== currentQ) {
      if (query) params.set('q', query);
      else params.delete('q');
      changed = true;
    }

    if (changed) {
      setSearchParams(params, { replace: true });
    }
  }, [filters.tagQuery, filters.projectNames, query]);

  const availablePlugins = useMemo(() => {
    const seen = new Set<string>();
    return allDocuments.reduce<{ id: string; label: string; icon: string }[]>((acc, d) => {
      const short = d.source_type.replace(/^com\.doxus\./, '');
      if (!seen.has(short)) {
        seen.add(short);
        acc.push({ id: short, label: short.charAt(0).toUpperCase() + short.slice(1), icon: getEmoji(`com.doxus.${short}`) });
      }
      return acc;
    }, []);
  }, [allDocuments, getEmoji]);

  const availableProjects = useMemo(() => {
    const seen = new Set<string>();
    return allDocuments.reduce<string[]>((acc, d) => {
      if (d.project_name && !seen.has(d.project_name)) { seen.add(d.project_name); acc.push(d.project_name); }
      return acc;
    }, []);
  }, [allDocuments]);

  const fetchPreview = async (doc: DocEntry, forceRefresh = false) => {
    setPreviewLoading(true);
    setPreviewError(null);
    try {
      const result = await invoke<any>('get_document_content', {
        filePath: doc.source_doc_id,
        projectName: doc.project_name || undefined,
        forceRefresh,
      });
      setPreviewContent(stripFrontmatter(result.content));
      setPreviewMeta(result);

      const newMeta = {
        title: result.title || doc.title,
        tags: result.tags || [],
        updated_at: result.updated_at || doc.updated_at,
        last_indexed: result.last_indexed || doc.last_indexed,
      };

      updateDocumentMetadata(doc.source_doc_id, newMeta);
      setSelectedDoc(prev => prev ? { ...prev, ...newMeta } : null);
    } catch (e) {
      setPreviewError(String(e));
    } finally {
      setPreviewLoading(false);
    }
  };

  const handleSelectDoc = async (doc: DocEntry) => {
    setSelectedDoc(doc);
    setPreviewContent(null);
    setPreviewMeta(null);
    const newId = doc.document_id.toString();
    processedDocIdRef.current = newId;
    setSearchParams({ docId: newId }, { replace: true });
    if (doc.document_id) {
      invoke('increment_view_count', { documentId: doc.document_id }).catch(() => { });
    }
    await fetchPreview(doc);
  };

  const activeFilterCount = filters.sourceTypes.length + filters.projectNames.length + (filters.tagQuery ? 1 : 0);
  const hasSearch = query.trim().length > 0;

  const groupedEntries = useMemo(() => {
    const entries = hasSearch ? hits.map(hitToEntry) : allDocuments.map(allDocToEntry);
    const groups = new Map<string, { sourceType: string; docs: DocEntry[] }>();
    for (const entry of entries) {
      const key = entry.project_name || '(No Project)';
      if (!groups.has(key)) groups.set(key, { sourceType: entry.source_type, docs: [] });
      groups.get(key)!.docs.push(entry);
    }
    return groups;
  }, [hits, allDocuments, hasSearch]);

  return (
    <div className="flex flex-col h-full gap-6 animate-in fade-in duration-500">
      <SearchHeader
        inputValue={inputValue}
        setInputValue={setInputValue}
        onSubmit={(e) => {
          e.preventDefault();
          setQuery(inputValue);
          setSelectedDoc(null);
          search();
        }}
        onClear={() => {
          clear();
          setSelectedDoc(null);
          setInputValue('');
        }}
        advancedOpen={advancedOpen}
        setAdvancedOpen={setAdvancedOpen}
        activeFilterCount={activeFilterCount}
        isLoading={isLoading}
        hasQuery={hasSearch}
      />

      {advancedOpen && (
        <AdvancedSearchPanel
          filters={filters}
          availablePlugins={availablePlugins}
          availableProjects={availableProjects}
          onChange={setFilters}
        />
      )}

      {error && (
        <div className="p-4 bg-red-500/10 border border-red-500/20 rounded-2xl text-red-400 text-xs font-bold animate-in slide-in-from-top duration-300">
          ⚠️ {error}
        </div>
      )}

      <div className="flex-1 flex gap-6 overflow-hidden min-h-0">
        <SearchSidebar
          isLoading={isLoading || allDocsLoading}
          itemCount={hasSearch ? hits.length : allDocuments.length}
          groupedEntries={groupedEntries}
          selectedDoc={selectedDoc}
          onSelect={handleSelectDoc}
          hasSearch={hasSearch}
        />

        <div className="flex-1 glass-card border-white/5 rounded-[2.5rem] overflow-hidden shadow-2xl relative flex flex-col">
          <SearchPreview
            selectedDoc={selectedDoc}
            previewContent={previewContent}
            previewMeta={previewMeta}
            previewLoading={previewLoading}
            previewError={previewError}
            onRefresh={() => selectedDoc && fetchPreview(selectedDoc, true)}
            onClose={() => {
              setSelectedDoc(null);
              setSearchParams({}, { replace: true });
            }}
            onTagClick={(tag) => {
              setFilters({ tagQuery: tag });
              search();
              setSelectedDoc(null);
              setAdvancedOpen(true);
            }}
          />
        </div>
      </div>
    </div>
  );
}
