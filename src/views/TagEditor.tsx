import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { DocumentKind, SessionSuggestion } from "../types";
import { MAX_TAGS, normalizeTag, suggestTags, tagCounts } from "../tags";
import { useDocumentTags } from "./TagTree";

interface EditorProps {
  tags: string[];
  /** 쓰인 태그와 문서 수(tagCounts). 자동완성 후보 */
  allTags: [string, number][];
  onChange: (tags: string[]) => void;
  /** AI가 붙인 뒤 사람이 고치지 않았다 */
  aiTagged?: boolean;
  disabled?: boolean;
}

/** 문서 태그 편집. 칩으로 보여 주고, 입력하면 쓰던 태그를 초성까지 찾아 제안한다
 * Enter나 쉼표로 붙이고, 칩의 ×나 빈 칸에서 Backspace로 뺀다. 자동완성은 화살표로 고른 경우에만 그 제안을 붙인다
 * @see docs/organize.md */
export function TagEditor({ tags, allTags, onChange, aiTagged, disabled }: EditorProps) {
  const [query, setQuery] = useState("");
  const [focused, setFocused] = useState(false);
  /** 화살표로 고른 제안. -1이면 고르지 않았고 Enter는 친 글자 그대로 붙인다 */
  const [active, setActive] = useState(-1);
  const suggestions = useMemo(() => suggestTags(allTags, query, tags), [allTags, query, tags]);
  const showList = focused && suggestions.length > 0;
  const full = tags.length >= MAX_TAGS;

  const add = (raw: string) => {
    const tag = normalizeTag(raw);
    setQuery("");
    setActive(-1);
    if (tag && !full && !tags.some((t) => t.toLowerCase() === tag.toLowerCase())) onChange([...tags, tag]);
  };

  return (
    <div className="tag-editor">
      {tags.map((t) => (
        <span key={t} className="tag-chip">
          <span className="tag-chip-text">{t}</span>
          <button type="button" aria-label={`${t} 태그 빼기`} disabled={disabled} onClick={() => onChange(tags.filter((x) => x !== t))}>
            ×
          </button>
        </span>
      ))}
      <span className="tag-input-wrap">
        <input
          className="tag-input"
          value={query}
          placeholder={full ? `태그는 ${MAX_TAGS}개까지` : tags.length ? "태그 추가" : "태그 추가 (분야/냉각처럼 /로 나눔)"}
          aria-label="태그 추가"
          aria-autocomplete="list"
          disabled={disabled || full}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(-1);
          }}
          onFocus={() => setFocused(true)}
          // 제안을 누르는 동안 목록이 사라지지 않도록 조금 늦게 닫는다
          onBlur={() => setTimeout(() => setFocused(false), 150)}
          onKeyDown={(e) => {
            // WebKit은 한글 조합을 확정하는 Enter를 isComposing 없이 keyCode 229로 보낸다
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (e.key === "ArrowDown" && showList) {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, suggestions.length - 1));
            } else if (e.key === "ArrowUp" && showList) {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, -1));
            } else if (e.key === "Enter" || e.key === ",") {
              e.preventDefault();
              add(showList && active >= 0 && e.key === "Enter" ? suggestions[active] : query);
            } else if (e.key === "Backspace" && !query && tags.length) {
              onChange(tags.slice(0, -1));
            } else if (e.key === "Escape") {
              setQuery("");
            }
          }}
        />
        {showList && (
          <ul className="tag-suggestions" role="listbox">
            {suggestions.map((s, i) => (
              <li key={s}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === active}
                  className={i === active ? "active" : ""}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => add(s)}
                >
                  {s}
                </button>
              </li>
            ))}
          </ul>
        )}
      </span>
      {aiTagged && tags.length > 0 && (
        <span className="tag-ai" title="AI가 붙인 태그입니다. 고치면 AI가 다시 덮어쓰지 않습니다">
          AI
        </span>
      )}
    </div>
  );
}

interface OrganizeProps {
  documentId: string;
  kind: DocumentKind;
  /** 지금 묶음. 비었을 때만 캘린더 추천을 보인다 */
  session: string | null;
  tags: string[];
  tagsByUser: boolean;
  ready: boolean;
  /** 받아들인 묶음을 저장한다(DocumentView saveEdits) */
  onAcceptSession: (session: string) => void;
  /** 태그를 저장했거나 AI가 붙였다. 문서 화면이 edits를 다시 읽는다 */
  onChanged: () => void;
  onError: (e: string) => void;
}

/** 문서 화면의 "정리" 패널: 태그 편집, AI 태그 추천(누를 때만), 캘린더 일정으로 묶음 추천(제안만)
 * @see docs/organize.md */
export function DocumentOrganize({ documentId, kind, session, tags, tagsByUser, ready, onAcceptSession, onChanged, onError }: OrganizeProps) {
  const library = useDocumentTags();
  const allTags = useMemo(() => tagCounts(Object.values(library)), [library]);
  const [local, setLocal] = useState(tags);
  const [busy, setBusy] = useState<"tags" | "calendar" | null>(null);
  const [suggested, setSuggested] = useState<string[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const [sessionHint, setSessionHint] = useState<SessionSuggestion | null>(null);

  // 문서 화면이 다시 읽은 값을 따른다. 배열은 매번 새로 오므로 내용으로 비교한다
  const tagsKey = tags.join("\n");
  useEffect(() => setLocal(tags), [tagsKey]);
  // 팜플렛 자동 태그(요약 직후)가 끝나면 문서 화면이 다시 읽는다(src-tauri/src/tagger.rs organize_after_summary)
  const changedRef = useRef(onChanged);
  changedRef.current = onChanged;
  useEffect(() => {
    const unlisten = listen<{ documentId: string }>("tags-changed", (e) => {
      if (e.payload.documentId === documentId) changedRef.current();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [documentId]);
  useEffect(() => {
    setSuggested([]);
    setNote(null);
    setSessionHint(null);
  }, [documentId]);

  const save = async (next: string[]) => {
    setLocal(next);
    try {
      // 문서 화면은 tags-changed 이벤트로 다시 읽는다
      setLocal(await api.setDocumentTags(documentId, next));
      setSuggested((s) => s.filter((t) => !next.includes(t)));
    } catch (e) {
      setLocal(tags);
      onError(String(e));
    }
  };

  const suggest = async () => {
    setBusy("tags");
    setNote(null);
    try {
      const result = await api.suggestTags(documentId);
      if (result.applied) {
        setSuggested([]);
        setNote("AI가 태그를 붙였습니다. 고치면 다시 덮어쓰지 않습니다.");
      } else {
        const fresh = result.tags.filter((t) => !local.includes(t));
        setSuggested(fresh);
        setNote(fresh.length ? "손으로 고친 태그는 그대로 둡니다. 붙일 태그를 누르세요." : "더 붙일 태그를 찾지 못했습니다.");
      }
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const findSession = async () => {
    setBusy("calendar");
    try {
      setSessionHint(await api.suggestSession(documentId));
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const hasSession = !!session?.trim();
  return (
    <section className="panel organize">
      <h3>태그</h3>
      <TagEditor tags={local} allTags={allTags} onChange={save} aiTagged={!tagsByUser} />
      <div className="button-row">
        <button className="secondary" disabled={!ready || busy !== null} onClick={suggest}>
          {busy === "tags" ? "태그 고르는 중" : "AI로 태그 추천"}
        </button>
      </div>
      {suggested.length > 0 && (
        <div className="chips tag-suggested" aria-label="AI 추천 태그">
          {suggested.map((t) => (
            <button key={t} className="chip" onClick={() => save([...local, t])}>
              + {t}
            </button>
          ))}
        </div>
      )}
      <p className="hint">
        {note ??
          (kind === "pamphlet"
            ? "팜플렛은 AI 정리가 끝나면 분야 태그를 자동으로 붙입니다."
            : "책과 일반 문서는 누를 때만 AI(구독 CLI)로 보냅니다.")}
      </p>

      {!hasSession && (
        <div className="session-suggest">
          <h3>묶음 찾기</h3>
          {!sessionHint && (
            <>
              <button className="secondary" disabled={!ready || busy !== null} onClick={findSession}>
                {busy === "calendar" ? "일정 확인 중" : "일정에서 묶음 찾기"}
              </button>
              <p className="hint">찍은 시각과 겹치는 캘린더 일정을 찾고, AI가 글자로 맞는지 확인합니다. 처음 누르면 캘린더 허락을 묻습니다.</p>
            </>
          )}
          {sessionHint && (
            <SessionHint
              hint={sessionHint}
              onAccept={(name) => {
                setSessionHint(null);
                onAcceptSession(name);
              }}
              onDismiss={() => setSessionHint(null)}
            />
          )}
        </div>
      )}
    </section>
  );
}

function formatRange(start: string, end: string, allDay: boolean): string {
  const s = new Date(start);
  const e = new Date(end);
  if (allDay) {
    // 종일 일정의 끝은 다음 날 0시라 하루 빼서 보인다
    const last = new Date(e.getTime() - 1);
    const day = (d: Date) => d.toLocaleDateString("ko-KR", { month: "numeric", day: "numeric" });
    return day(s) === day(last) ? `${day(s)} 종일` : `${day(s)}~${day(last)}`;
  }
  const time = (d: Date) => d.toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" });
  return `${s.toLocaleDateString("ko-KR", { month: "numeric", day: "numeric" })} ${time(s)}~${time(e)}`;
}

function SessionHint({ hint, onAccept, onDismiss }: { hint: SessionSuggestion; onAccept: (s: string) => void; onDismiss: () => void }) {
  return (
    <div className="session-hint">
      {hint.session ? (
        <p>
          제안: <strong>{hint.session}</strong>
        </p>
      ) : (
        <p className="hint">{hint.reason || "맞는 일정을 찾지 못했습니다."}</p>
      )}
      {hint.session && hint.reason && <p className="hint">{hint.reason}</p>}
      {hint.candidates.length > 0 && (
        <ul className="plain-list">
          {hint.candidates.map((c, i) => (
            <li key={`${c.title}-${c.start}`}>
              <span>
                {c.title}
                <span className="hint"> {formatRange(c.start, c.end, c.allDay)}</span>
              </span>
              {i !== hint.chosen && (
                <button className="ghost" onClick={() => onAccept(c.title.trim())} title="AI가 고르지 않은 일정입니다. 맞으면 직접 고르세요">
                  이걸로
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="button-row">
        {hint.session && (
          <button className="primary" onClick={() => onAccept(hint.session!)}>
            묶음으로 정하기
          </button>
        )}
        <button className="ghost" onClick={onDismiss}>
          닫기
        </button>
      </div>
    </div>
  );
}
