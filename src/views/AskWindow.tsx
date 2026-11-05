import { useEffect, useMemo, useRef, useState } from "react";
import type { DocumentSummary } from "../types";
import { allTagPaths, UNTAGGED } from "../tags";
import { AskPanel } from "./AskPanel";
import { readTagFilter, useDocumentTags } from "./TagTree";

const OPEN_EVENT = "leaf:open-ask";

/** ⌘J 질문 창을 연다. 단축키, 빠른 열기, 메뉴 어디서든 부른다 */
export function openAsk() {
  window.dispatchEvent(new CustomEvent(OPEN_EVENT));
}

interface Props {
  documents: DocumentSummary[];
  /** 답의 인용을 눌렀을 때. 창을 닫고 그 문서 페이지를 연다 */
  onOpen: (documentId: string, outputId: string | null) => void;
}

/** 모든 문서에 묻는 창(⌘J). 범위를 태그(하위 포함)와 묶음으로 좁힌다. 처음 태그는 보관함에서 보던 태그
 * @see docs/ask.md */
export function AskWindow({ documents, onOpen }: Props) {
  const [open, setOpen] = useState(false);
  // 한 번 연 뒤에는 닫아도 대화와 기다리던 답을 지키려고 숨기기만 한다
  const [opened, setOpened] = useState(false);
  const openRef = useRef(false);
  openRef.current = open;
  const library = useDocumentTags(open);
  const tagPaths = useMemo(() => allTagPaths(Object.values(library)), [library]);
  const sessions = useMemo(
    () => [...new Set(documents.map((d) => d.session?.trim()).filter((s): s is string => !!s))].sort((a, b) => a.localeCompare(b, "ko")),
    [documents],
  );
  const [tag, setTag] = useState<string | null>(null);
  const [session, setSession] = useState<string | null>(null);

  useEffect(() => {
    const show = () => {
      const seen = readTagFilter();
      setTag(seen && seen !== UNTAGGED ? seen : null);
      setOpened(true);
      setOpen(true);
    };
    // ⌘J. 다른 단축키는 App 쪽 담당이 정하고, 이 창은 openAsk()로도 열린다
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "j") {
        e.preventDefault();
        if (openRef.current) setOpen(false);
        else show();
      } else if (e.key === "Escape" && openRef.current) setOpen(false);
    };
    window.addEventListener(OPEN_EVENT, show);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener(OPEN_EVENT, show);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  if (!opened) return null;
  return (
    <div className="modal" hidden={!open} onClick={(e) => e.target === e.currentTarget && setOpen(false)}>
      <div className="modal-body ask-window" role="dialog" aria-label="모든 문서에 질문">
        <div className="modal-head">
          <h3>모든 문서에 질문</h3>
          <span className="spacer" />
          <select aria-label="태그 범위" value={tag ?? ""} onChange={(e) => setTag(e.target.value || null)}>
            <option value="">모든 태그</option>
            {tag && !tagPaths.includes(tag) && <option value={tag}>{tag}</option>}
            {tagPaths.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
          <select aria-label="묶음 범위" value={session ?? ""} onChange={(e) => setSession(e.target.value || null)}>
            <option value="">모든 묶음</option>
            {sessions.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
          <button className="ghost" onClick={() => setOpen(false)}>
            닫기
          </button>
        </div>
        <AskPanel
          documentId={null}
          scopeTag={tag}
          scopeSession={session}
          onCite={(documentId, outputId) => {
            setOpen(false);
            onOpen(documentId, outputId);
          }}
        />
      </div>
    </div>
  );
}
