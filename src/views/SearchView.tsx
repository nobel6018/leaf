import { useEffect, useState } from "react";
import { api, KIND_LABEL } from "../api";
import type { DocumentHits } from "../types";

interface Props {
  onOpen: (documentId: string, outputId?: string, query?: string) => void;
  onError: (e: string) => void;
}

/** 본문(OCR)은 부분 문자열, 문서 이름과 세션은 초성(ㅂㅅㅎ)까지 찾는다 */
export function SearchView({ onOpen, onError }: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<DocumentHits[] | null>(null);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults(null);
      return;
    }
    // 글자를 칠 때마다 묻지 않고 잠깐 멈추면 찾는다
    const timer = window.setTimeout(() => {
      api
        .search(q)
        .then(setResults)
        .catch((e) => onError(String(e)));
    }, 150);
    return () => window.clearTimeout(timer);
  }, [query, onError]);

  return (
    <section className="page">
      <h1>검색</h1>
      <input
        className="search-input"
        autoFocus
        placeholder="글자, 문서 이름, 초성(예: ㅎㅂㄹ)"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {results && results.length === 0 && <p className="empty">찾은 문서가 없습니다</p>}
      {results?.map((doc) => (
        <div className="result" key={doc.documentId}>
          <button className="result-head" onClick={() => onOpen(doc.documentId, undefined, query)}>
            <span className={`badge kind-${doc.kind}`}>{KIND_LABEL[doc.kind]}</span>
            <span className="result-name">{doc.name}</span>
            {doc.session && <span className="hint">{doc.session}</span>}
            {doc.nameMatch && <span className="badge state-ok">이름</span>}
            {doc.totalPages > 0 && <span className="hint">{doc.totalPages}쪽에서 찾음</span>}
          </button>
          {doc.hits.map((hit) => (
            <button key={hit.outputId} className="hit" onClick={() => onOpen(doc.documentId, hit.outputId, query)}>
              <span className="hit-page">{hit.pageNumber}쪽</span>
              <span className="hit-text">
                {hit.snippet.map((s, i) => (s.hit ? <mark key={i}>{s.text}</mark> : <span key={i}>{s.text}</span>))}
              </span>
            </button>
          ))}
          {doc.totalPages > doc.hits.length && <p className="hint">외 {doc.totalPages - doc.hits.length}쪽</p>}
        </div>
      ))}
    </section>
  );
}
