/**
 * MediaWiki API type definitions
 */

/** API response wrapper */
export interface ApiResponse {
  error?: ApiError;
  warnings?: Record<string, { '*': string }>;
}

/** API error */
export interface ApiError {
  code: string;
  info: string;
  docref?: string;
}

/** Query response */
export interface QueryResponse {
  query: {
    pages?: Record<string, PageInfo>;
    allpages?: AllPagesItem[];
    categorymembers?: CategoryMember[];
    search?: SearchResult[];
    recentchanges?: RecentChange[];
    querypage?: {
      name?: string;
      results?: QueryPageResult[];
    };
    tokens?: Record<string, string>;
  };
  continue?: Record<string, string>;
  batchcomplete?: boolean;
}

/** Page info from query */
export interface PageInfo {
  pageid?: number;
  ns: number;
  title: string;
  missing?: boolean;
  revisions?: Revision[];
  contentmodel?: string;
}

/** Revision data */
export interface Revision {
  revid: number;
  parentid?: number;
  timestamp: string;
  user?: string;
  comment?: string;
  slots?: {
    main: {
      contentmodel: string;
      contentformat: string;
      content: string; // formatversion=2 uses 'content', formatversion=1 uses '*'
    };
  };
}

/** AllPages item */
export interface AllPagesItem {
  pageid: number;
  ns: number;
  title: string;
}

/** Category member */
export interface CategoryMember {
  pageid: number;
  ns: number;
  title: string;
}

/** Search result */
export interface SearchResult {
  ns: number;
  title: string;
  pageid: number;
  size: number;
  wordcount: number;
  snippet: string;
  timestamp: string;
}

/** Recent change */
export interface RecentChange {
  type: string;
  ns: number;
  title: string;
  pageid: number;
  revid: number;
  old_revid: number;
  timestamp: string;
  user?: string;
}

/** Login response */
export interface LoginResponse {
  login: {
    result: 'Success' | 'NeedToken' | 'Failed' | 'Aborted';
    lguserid?: number;
    lgusername?: string;
    reason?: string;
  };
}

/** Edit response */
export interface EditResponse {
  edit: {
    result: 'Success' | 'Failure';
    pageid?: number;
    title?: string;
    contentmodel?: string;
    oldrevid?: number;
    newrevid?: number;
    newtimestamp?: string;
    nochange?: boolean;
  };
}

/** Delete response */
export interface DeleteResponse {
  delete: {
    title: string;
    reason: string;
    logid: number;
  };
}

/** Token types */
export type TokenType = 'login' | 'csrf' | 'watch' | 'patrol' | 'rollback' | 'userrights';

/** Batch query options */
export interface BatchOptions {
  /** Maximum items per batch (default: 50 for most, 500 for generator queries) */
  batchSize?: number;
  /** Callback for progress reporting */
  onProgress?: (completed: number, total: number) => void;
}

/** Page content with metadata */
export interface PageContent {
  title: string;
  content: string;
  timestamp: string;
  revisionId: number;
  pageId: number;
  contentModel: string;
  namespace: number;
}

/** Page timestamp only (for conflict detection) */
export interface PageTimestamp {
  title: string;
  timestamp: string;
  revisionId: number;
}

/** QueryPage result item */
export interface QueryPageResult {
  ns: number;
  title: string;
  value?: number | string;
}
