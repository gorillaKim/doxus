import { SearchHit, AllDocument } from '../stores/useSearchStore';
import { DocEntry } from '../components/search/SearchTree';

export function stripFrontmatter(content: string): string {
  if (!content.startsWith('---')) return content;
  const end = content.indexOf('\n---', 3);
  if (end === -1) return content;
  return content.slice(end + 4).trimStart();
}

export function hitToEntry(hit: SearchHit): DocEntry {
  return {
    document_id: hit.document_id,
    chunk_id: hit.chunk_id,
    title: hit.title ?? 'Untitled',
    source_doc_id: hit.source_doc_id ?? String(hit.document_id),
    hierarchy_path: hit.file_path ?? hit.source_doc_id ?? '',
    project_name: hit.project_name ?? '',
    source_type: hit.source_type ?? '',
    score: hit.score,
    snippet: hit.snippet ?? undefined,
    context_content: hit.context_content,
    heading_path: hit.heading_path,
    tags: hit.tags,
    updated_at: hit.updated_at,
    last_indexed: hit.last_indexed,
    cache_ttl: hit.cache_ttl,
    metadata: hit.metadata,
    url: hit.url,
    source_project_id: hit.source_project_id,
    freshness_score: hit.freshness_score,
    retention_tier: hit.retention_tier,
  };
}

export function allDocToEntry(doc: AllDocument): DocEntry {
  return {
    document_id: doc.document_id,
    chunk_id: 0,
    title: doc.title,
    source_doc_id: doc.source_doc_id,
    hierarchy_path: doc.file_path || doc.source_doc_id,
    project_name: doc.project_name,
    source_type: doc.source_type,
    heading_path: null,
    tags: doc.tags,
    updated_at: doc.updated_at,
    last_indexed: doc.last_indexed,
    cache_ttl: doc.cache_ttl,
    url: doc.url,
    source_project_id: doc.source_project_id || '',
    freshness_score: doc.freshness_score,
    retention_tier: doc.retention_tier,
  };
}
