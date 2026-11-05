import { Fragment, ReactNode, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { AskAnswer, AskSource } from "../types";

interface Props {
  /** 없으면 모든 문서(scope 안)에 묻는다 */
  documentId: string | null;
  /** 모든 문서에 물을 때 범위. 태그(하위 포함)와 묶음 */
  scopeTag?: string | null;
  scopeSession?: string | null;
  /** 답의 페이지 표시를 눌렀을 때. outputId가 없으면 문서 첫 화면 */
  onCite: (documentId: string, outputId: string | null) => void;
}

interface Turn {
  question: string;
  answer: AskAnswer | null;
  error: string | null;
}

/** 대괄호 안 인용 하나: "p.12", "D3 p.12", "D3", "D3 p12" */
const CITE_PART = /^(D\d+)?\s*(?:p\.?\s*(\d+))?$/i;
/** 뒤에 (가 붙은 대괄호는 원래 마크다운 링크라 인용으로 보지 않는다 */
const INLINE = /\[([^[\]\n]{1,80})\](?!\()|\[([^[\]\n]+)\]\([^)\s]*\)|\*\*([^*]+)\*\*|`([^`]+)`/g;

interface Cite {
  key: string | null;
  page: number | null;
}

/** "[D3 p.2, p.4]"처럼 한 괄호에 여러 개를 묶어도 하나씩 읽는다. 문서 표시가 빠진 것은 앞 문서를 잇는다. 인용이 아니면 null */
export function parseCitation(inner: string): Cite[] | null {
  let last: string | null = null;
  const out: Cite[] = [];
  for (const raw of inner.split(/[,;]\s*/)) {
    const m = raw.trim().match(CITE_PART);
    if (!m || (!m[1] && !m[2])) return null;
    const key: string | null = m[1] ? m[1].toUpperCase() : last;
    last = key;
    out.push({ key, page: m[2] ? Number(m[2]) : null });
  }
  return out;
}

function sourceOf(sources: AskSource[], key: string | null): AskSource | undefined {
  return key ? sources.find((s) => s.key === key) : sources.length === 1 ? sources[0] : undefined;
}

/** 답 글자 렌더링. 마크다운 라이브러리 없이 제목, 목록, 굵게, 인용만 그린다. 답에 섞인 일반 링크는 글자로만 보인다 */
function renderInline(text: string, sources: AskSource[], onCite: Props["onCite"], single: boolean): ReactNode[] {
  const out: ReactNode[] = [];
  let at = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > at) out.push(text.slice(at, m.index));
    at = m.index + m[0].length;
    const [whole, cite, linkText, bold, code] = m;
    if (cite !== undefined) {
      const parts = parseCitation(cite);
      if (!parts) {
        out.push(whole);
        continue;
      }
      parts.forEach((c, i) => {
        const source = sourceOf(sources, c.key);
        const outputId = source && c.page ? (source.pages[c.page - 1] ?? null) : null;
        const label = [single ? null : source?.name ?? c.key, c.page ? `p.${c.page}` : null].filter(Boolean).join(" ");
        out.push(
          source ? (
            <button key={`${m.index}-${i}`} className="cite" title={source.name} onClick={() => onCite(source.documentId, outputId)}>
              {label}
            </button>
          ) : (
            <span key={`${m.index}-${i}`} className="cite missing">
              {label || whole}
            </span>
          ),
        );
      });
    } else if (linkText !== undefined) out.push(linkText);
    else if (bold !== undefined) out.push(<strong key={m.index}>{bold}</strong>);
    else if (code !== undefined) out.push(<code key={m.index}>{code}</code>);
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

function AnswerBody({ answer, onCite, single }: { answer: AskAnswer; onCite: Props["onCite"]; single: boolean }) {
  const blocks: ReactNode[] = [];
  let list: { ordered: boolean; items: string[] } | null = null;
  let para: string[] = [];
  const inline = (t: string) => renderInline(t, answer.sources, onCite, single);
  const flush = () => {
    if (para.length) {
      blocks.push(<p key={blocks.length}>{inline(para.join(" "))}</p>);
      para = [];
    }
    if (list) {
      const items = list.items.map((t, i) => <li key={i}>{inline(t)}</li>);
      blocks.push(list.ordered ? <ol key={blocks.length}>{items}</ol> : <ul key={blocks.length}>{items}</ul>);
      list = null;
    }
  };
  for (const raw of answer.markdown.split("\n")) {
    const line = raw.trim();
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    const bullet = line.match(/^[-*]\s+(.*)$/);
    const numbered = line.match(/^\d+[.)]\s+(.*)$/);
    if (!line || line === "```") flush();
    else if (heading) {
      flush();
      blocks.push(<h4 key={blocks.length}>{inline(heading[1])}</h4>);
    } else if (bullet || numbered) {
      const ordered = !!numbered;
      if (para.length || (list && list.ordered !== ordered)) flush();
      if (!list) list = { ordered, items: [] };
      list.items.push((bullet ?? numbered)![1]);
    } else if (list) list.items[list.items.length - 1] += ` ${line}`;
    else para.push(line);
  }
  flush();
  return <div className="ask-answer">{blocks.map((b, i) => <Fragment key={i}>{b}</Fragment>)}</div>;
}

const EXAMPLES_ONE = ["핵심 내용을 세 줄로 알려 줘", "숫자나 스펙만 모아 줘", "연락처나 웹사이트가 있어?"];
const EXAMPLES_ALL = ["냉각 방식을 다룬 자료를 모아 줘", "로봇을 만드는 회사는 어디야?", "가격이 적힌 문서가 있어?"];

/** 문서에 질문하기. 사람이 보낼 때만 구독 CLI로 보낸다(src-tauri/src/ask.rs)
 * @see docs/ask.md */
export function AskPanel({ documentId, scopeTag = null, scopeSession = null, onCite }: Props) {
  const [question, setQuestion] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [running, setRunning] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);
  const single = documentId !== null;

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [turns]);

  const ask = async (q: string) => {
    const text = q.trim();
    if (!text || running) return;
    setQuestion("");
    setRunning(true);
    setTurns((prev) => [...prev, { question: text, answer: null, error: null }]);
    const settle = (patch: Partial<Turn>) => setTurns((prev) => prev.map((t, i) => (i === prev.length - 1 ? { ...t, ...patch } : t)));
    try {
      const answer = documentId ? await api.askDocument(documentId, text) : await api.askLibrary(text, scopeTag, scopeSession);
      settle({ answer });
    } catch (e) {
      settle({ error: String(e) });
    } finally {
      setRunning(false);
    }
  };

  const scopeLabel = [scopeTag && `태그 '${scopeTag}'`, scopeSession && `묶음 '${scopeSession}'`].filter(Boolean).join(", ");
  return (
    <section className="ask">
      <div className="ask-turns">
        {turns.length === 0 && (
          <div className="ask-empty">
            <p className="hint">
              {single
                ? "이 문서의 글자(OCR)와 AI 정리를 근거로 답합니다. 답의 페이지 표시를 누르면 그 페이지로 갑니다."
                : "문서마다 AI 정리(없으면 앞부분)와, 질문 낱말이 나온 글을 근거로 답합니다. 문서에 실제로 나온 말로 물을수록 정확합니다."}
            </p>
            <div className="chips">
              {(single ? EXAMPLES_ONE : EXAMPLES_ALL).map((ex) => (
                <button key={ex} className="chip" onClick={() => ask(ex)}>
                  {ex}
                </button>
              ))}
            </div>
          </div>
        )}
        {turns.map((t, i) => (
          <div key={i} className="ask-turn">
            <p className="ask-question">{t.question}</p>
            {t.error && <p className="ask-error">{t.error}</p>}
            {!t.answer && !t.error && <p className="hint">답을 찾는 중입니다. 30초에서 1분쯤 걸립니다.</p>}
            {t.answer && <AnswerBody answer={t.answer} onCite={onCite} single={single} />}
          </div>
        ))}
        <div ref={endRef} />
      </div>
      <form
        className="ask-form"
        onSubmit={(e) => {
          e.preventDefault();
          ask(question);
        }}
      >
        <input
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder={single ? "이 문서에 물어보기" : scopeLabel ? `${scopeLabel} 문서에 물어보기` : "모든 문서에 물어보기"}
          aria-label="질문"
          disabled={running}
        />
        <button className="primary" type="submit" disabled={running || !question.trim()}>
          {running ? "묻는 중" : "묻기"}
        </button>
      </form>
      <p className="hint ask-foot">질문을 보낼 때만 문서 글자를 구독 CLI(claude)로 보냅니다.</p>
    </section>
  );
}
