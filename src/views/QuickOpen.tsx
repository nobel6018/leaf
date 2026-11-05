import { KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, compact, fileUrl, KIND_LABEL } from "../api";
import { IS_MAC } from "../shortcuts";
import type { DocumentDetail, DocumentHits, OcrPage, PageHit, QuickOpenEntry } from "../types";

// 빠른 찾기(⌘K). 문서 이름, 묶음, 태그는 초성까지 화면에서 맞추고, 본문(OCR)은 search_library로 찾는다.
// 왼쪽에서 고르면 오른쪽에 그 쪽 그림과 글자를 미리 보여 주고, 열지 않고 마크다운으로 복사할 수 있다
// @see docs/shortcuts-and-deep-links.md

interface Props {
  initialQuery: string;
  onClose: () => void;
  /** outputId가 있으면 그 쪽을, query가 있으면 그 글자를 칠해서 연다 */
  onOpen: (documentId: string, outputId?: string, query?: string) => void;
}

type MatchField = "이름" | "묶음" | "태그";

interface Row {
  key: string;
  entry: QuickOpenEntry;
  /** 본문에서 찾은 쪽. 문서 줄이면 없음 */
  hit?: PageHit;
  matched?: MatchField[];
  /** 문서 줄에서, 보여 주지 않은 본문 쪽 수 */
  moreHits?: number;
}

const MOD = IS_MAC ? "⌘" : "Ctrl+";
/** 문서 하나에서 목록에 펼칠 본문 쪽 수. 나머지는 개수만 */
const HITS_SHOWN = 5;

/** 한글 초성. src-tauri/src/search.rs INITIALS와 같다 */
const INITIALS = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ";

function initialOf(c: string): string | null {
  const code = c.charCodeAt(0);
  return code >= 0xac00 && code <= 0xd7a3 ? INITIALS[Math.floor((code - 0xac00) / 588)] : null;
}

/** 초성을 섞은 검색어로 부분 문자열을 찾는다(ㅎㅂㄹ, 한ㅂ). 둘 다 compact를 거친 값. search.rs matches_with_initials와 같은 규칙 */
export function matchesWithInitials(hay: string, needle: string): boolean {
  if (!needle) return false;
  if (hay.includes(needle)) return true;
  const n = [...needle];
  if (!n.some((c) => INITIALS.includes(c))) return false;
  const h = [...hay];
  for (let i = 0; i + n.length <= h.length; i++) {
    if (n.every((c, j) => h[i + j] === c || (INITIALS.includes(c) && initialOf(h[i + j]) === c))) return true;
  }
  return false;
}

function matchFields(entry: QuickOpenEntry, needle: string): MatchField[] {
  const out: MatchField[] = [];
  if (matchesWithInitials(compact(entry.name), needle)) out.push("이름");
  if (entry.session && matchesWithInitials(compact(entry.session), needle)) out.push("묶음");
  if (entry.tags.some((t) => matchesWithInitials(compact(t), needle))) out.push("태그");
  return out;
}

export function QuickOpen({ initialQuery, onClose, onOpen }: Props) {
  const [query, setQuery] = useState(initialQuery);
  const [entries, setEntries] = useState<QuickOpenEntry[] | null>(null);
  const [ocr, setOcr] = useState<{ query: string; hits: DocumentHits[] } | null>(null);
  const [cursor, setCursor] = useState(0);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    api
      .quickOpenEntries()
      .then(setEntries)
      .catch((e) => setError(String(e)));
  }, []);

  // 본문 검색은 글자를 칠 때마다 하지 않고 잠깐 멈추면 한다
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setOcr(null);
      return;
    }
    let stale = false;
    const timer = window.setTimeout(() => {
      api
        .search(q)
        .then((hits) => !stale && setOcr({ query: q, hits }))
        .catch((e) => !stale && setError(String(e)));
    }, 150);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const rows = useMemo(() => {
    if (!entries) return [];
    const needle = compact(query);
    if (!needle) return entries.map((entry): Row => ({ key: entry.id, entry }));
    const byId = new Map(entries.map((e) => [e.id, e]));
    const ocrById = new Map((ocr?.hits ?? []).map((d) => [d.documentId, d]));
    const out: Row[] = [];
    const push = (entry: QuickOpenEntry, matched: MatchField[]) => {
      const found = ocrById.get(entry.id);
      const hits = found?.hits ?? [];
      out.push({
        key: entry.id,
        entry,
        matched,
        moreHits: Math.max(0, (found?.totalPages ?? 0) - Math.min(hits.length, HITS_SHOWN)),
      });
      hits.slice(0, HITS_SHOWN).forEach((hit) => out.push({ key: `${entry.id}:${hit.outputId}`, entry, hit }));
    };
    // 이름, 묶음, 태그가 맞은 문서를 최신순으로 먼저, 그다음 본문만 맞은 문서
    const seen = new Set<string>();
    for (const entry of entries) {
      const matched = matchFields(entry, needle);
      if (matched.length === 0) continue;
      seen.add(entry.id);
      push(entry, matched);
    }
    for (const doc of ocr?.hits ?? []) {
      const entry = byId.get(doc.documentId);
      if (!entry || seen.has(doc.documentId) || doc.hits.length === 0) continue;
      push(entry, []);
    }
    return out;
  }, [entries, ocr, query]);

  useEffect(() => setCursor(0), [query]);
  useEffect(() => {
    listRef.current?.querySelector(`[data-row="${cursor}"]`)?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const row = rows[Math.min(cursor, rows.length - 1)] as Row | undefined;

  const open = (r: Row | undefined) => {
    if (!r) return;
    onOpen(r.entry.id, r.hit?.outputId, r.hit ? query.trim() : undefined);
  };

  const copyMarkdown = async () => {
    if (!row) return;
    try {
      await navigator.clipboard.writeText(await api.documentMarkdown(row.entry.id));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      setError(String(e));
    }
  };

  // 창 전체에서 키를 받는다. 복사 단추를 누른 뒤에도 Esc와 방향키가 동작해야 한다
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    const mod = IS_MAC ? e.metaKey : e.ctrlKey;
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") setCursor((c) => Math.min(c + 1, rows.length - 1));
    else if (e.key === "ArrowUp") setCursor((c) => Math.max(c - 1, 0));
    else if (e.key === "Enter" && !mod && !e.shiftKey) open(row);
    else if (mod && e.shiftKey && e.key.toLowerCase() === "c") copyMarkdown();
    // 빠른 찾기가 열린 채 ⌘K를 다시 누르면 닫는다
    else if (mod && !e.shiftKey && e.key.toLowerCase() === "k") onClose();
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="quick-open" role="dialog" aria-label="빠른 찾기" onClick={(e) => e.stopPropagation()} onKeyDown={onKeyDown}>
        <input
          className="quick-input"
          autoFocus
          placeholder="문서 이름, 묶음, 태그, 본문 글자, 초성(ㅎㅂㄹ)"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="검색어"
        />
        <div className="quick-body">
          <ul className="quick-list" ref={listRef} role="listbox">
            {entries === null && <li className="hint quick-empty">여는 중</li>}
            {entries !== null && rows.length === 0 && (
              <li className="hint quick-empty">{query.trim() ? "찾은 문서가 없습니다" : "보관함이 비어 있습니다"}</li>
            )}
            {rows.map((r, i) => (
              <li
                key={r.key}
                data-row={i}
                role="option"
                aria-selected={i === cursor}
                className={`quick-row ${r.hit ? "hit" : "doc"} ${i === cursor ? "selected" : ""}`}
                onMouseMove={() => i !== cursor && setCursor(i)}
                onClick={() => open(r)}
              >
                {r.hit ? (
                  <>
                    <span className="quick-hit-page">{r.hit.pageNumber}쪽</span>
                    <span className="quick-hit-text">
                      {r.hit.snippet.map((s, j) => (s.hit ? <mark key={j}>{s.text}</mark> : <span key={j}>{s.text}</span>))}
                    </span>
                  </>
                ) : (
                  <>
                    <span className="quick-title">
                      <span className={`badge kind-${r.entry.kind}`}>{KIND_LABEL[r.entry.kind]}</span> {r.entry.name}
                    </span>
                    <span className="quick-meta">
                      {[
                        r.entry.session,
                        `${r.entry.pageCount}장`,
                        new Date(r.entry.createdAt).toLocaleDateString("ko-KR"),
                        r.matched?.length ? `${r.matched.join(", ")} 일치` : null,
                        r.moreHits ? `본문 ${r.moreHits}쪽 더` : null,
                      ]
                        .filter(Boolean)
                        .join(" | ")}
                    </span>
                  </>
                )}
              </li>
            ))}
          </ul>
          <Preview entry={row?.entry ?? null} outputId={row?.hit?.outputId} query={row?.hit ? query : ""} />
        </div>
        <footer className="quick-footer">
          {error && <span className="error-text">{error}</span>}
          <span className="hint">↑↓ 고르기 | ↩ 열기 | Esc 닫기</span>
          <span className="spacer" />
          <button className="ghost" onClick={copyMarkdown} disabled={!row}>
            {copied ? "복사했습니다" : `글자를 마크다운으로 복사 ⇧${MOD}C`}
          </button>
        </footer>
      </div>
    </div>
  );
}

/** 고른 쪽(문서 줄이면 첫 쪽)의 그림과 OCR 글자. 찾은 곳 근처만 보여 준다 */
function Preview({ entry, outputId, query }: { entry: QuickOpenEntry | null; outputId?: string; query: string }) {
  const [detail, setDetail] = useState<DocumentDetail | null>(null);
  const [text, setText] = useState<OcrPage | null>(null);
  const id = entry?.id ?? null;

  useEffect(() => {
    setDetail(null);
    if (!id) return;
    let stale = false;
    api
      .getDocument(id)
      .then((d) => !stale && setDetail(d))
      .catch(() => !stale && setDetail(null));
    return () => {
      stale = true;
    };
  }, [id]);

  const outputs = useMemo(() => detail?.pages.flatMap((p) => p.outputs) ?? [], [detail]);
  const output = outputs.find((o) => o.id === outputId) ?? outputs[0];

  useEffect(() => {
    setText(null);
    if (!id || !output?.hasOcr) return;
    let stale = false;
    api
      .getOcr(id, output.id)
      .then((t) => !stale && setText(t))
      .catch(() => !stale && setText(null));
    return () => {
      stale = true;
    };
  }, [id, output?.id, output?.hasOcr]);

  if (!entry) return <div className="quick-preview" />;
  const pageIndex = output ? outputs.indexOf(output) + 1 : 0;
  return (
    <div className="quick-preview">
      <h3>{entry.name}</h3>
      <p className="hint">
        {[entry.session, `${entry.pageCount}장`, pageIndex ? `${pageIndex}쪽 미리 보기` : "처리 전"].filter(Boolean).join(" | ")}
      </p>
      {entry.tags.length > 0 && (
        <div className="quick-tags">
          {entry.tags.map((t) => (
            <span className="badge" key={t}>
              #{t}
            </span>
          ))}
        </div>
      )}
      {output && (
        <div className="quick-image">
          <img src={fileUrl(output.thumb ?? output.image, output.processedAt)} alt="" />
        </div>
      )}
      {text && <p className="quick-text">{excerpt(text.text, query)}</p>}
      {output && !output.hasOcr && <p className="hint">글자를 아직 읽지 않았습니다</p>}
    </div>
  );
}

/** 찾은 곳 앞뒤 400자. 띄어쓰기를 무시하고 맞춘다(search.rs find_spans와 같은 규칙) */
function excerpt(text: string, query: string) {
  const needle = [...compact(query)];
  const chars = [...text];
  const LIMIT = 800;
  if (needle.length > 0) {
    const lower = chars.map((c) => c.toLowerCase());
    for (let start = 0; start < chars.length; start++) {
      if (/\s/.test(lower[start])) continue;
      let i = start;
      let n = 0;
      while (i < chars.length && n < needle.length) {
        if (/\s/.test(lower[i])) i++;
        else if (lower[i] === needle[n]) {
          i++;
          n++;
        } else break;
      }
      if (n === needle.length) {
        const from = Math.max(0, start - LIMIT / 2);
        const to = Math.min(chars.length, i + LIMIT / 2);
        return (
          <>
            {from > 0 && "…"}
            {chars.slice(from, start).join("")}
            <mark>{chars.slice(start, i).join("")}</mark>
            {chars.slice(i, to).join("")}
            {to < chars.length && "…"}
          </>
        );
      }
    }
  }
  return chars.length > LIMIT ? `${chars.slice(0, LIMIT).join("")}…` : text;
}
