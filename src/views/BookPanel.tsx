import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { PagePart } from "../types";

// src-tauri/src/book.rs 직렬화 모양과 같다 (docs/book.md)
export type NumberKind = "roman" | "arabic";

export interface DetectedNumber {
  value: number;
  kind: NumberKind;
  label: string;
  span: number;
  confidence: number;
  corrected: boolean;
  raw: string;
}

export interface OutputNumber {
  outputId: string;
  sourcePage: string;
  part: PagePart;
  hasOcr: boolean;
  number: DetectedNumber | null;
  duplicate: boolean;
  outOfOrder: boolean;
}

export interface Gap {
  kind: NumberKind;
  after: number;
  before: number;
  missingCount: number;
  missing: number[];
  unnumberedBetween: number;
  likelyUnnumbered: boolean;
}

export interface Duplicate {
  kind: NumberKind;
  value: number;
  outputIds: string[];
  sourcePages: string[];
}

export interface BookReport {
  outputs: OutputNumber[];
  detected: number;
  total: number;
  first: number | null;
  last: number | null;
  missing: number[];
  gaps: Gap[];
  duplicates: Duplicate[];
  outOfOrder: string[];
  sortMoves: number;
}

export interface SortResult {
  changed: boolean;
  moved: number;
}

interface Props {
  documentId: string;
  /** 처리 결과가 바뀌면 다시 읽는다 */
  revision: string;
  ready: boolean;
  /** 문제 페이지를 누르면 그 페이지로 간다 */
  onSelect: (outputId: string) => void;
  onSorted: () => void;
  onError: (e: string) => void;
}

function toRoman(n: number): string {
  const table: [number, string][] = [
    [40, "xl"],
    [10, "x"],
    [9, "ix"],
    [5, "v"],
    [4, "iv"],
    [1, "i"],
  ];
  let out = "";
  for (const [v, s] of table) {
    while (n >= v) {
      out += s;
      n -= v;
    }
  }
  return out;
}

/** 범위 글자: 59, 59~61 */
function range(from: number, to: number): string {
  return from === to ? `${from}` : `${from}~${to}`;
}

/** 책 쪽 번호 확인. 빠진 쪽, 두 번 찍은 쪽, 순서가 어긋난 쪽을 보여 주고, 누를 때만 쪽 번호 순서로 정렬한다 */
export function BookPanel({ documentId, revision, ready, onSelect, onSorted, onError }: Props) {
  const [report, setReport] = useState<BookReport | null>(null);
  const [sorting, setSorting] = useState(false);
  const [showAll, setShowAll] = useState(false);

  const reload = useCallback(() => {
    if (!ready) return;
    api
      .bookPageReport(documentId)
      .then(setReport)
      .catch((e) => onError(String(e)));
  }, [documentId, ready, onError]);

  useEffect(reload, [reload, revision]);

  const sort = async () => {
    setSorting(true);
    try {
      await api.sortByPageNumbers(documentId);
      onSorted();
      reload();
    } catch (e) {
      onError(String(e));
    } finally {
      setSorting(false);
    }
  };

  if (!ready) {
    return (
      <section className="panel book-panel">
        <h3>쪽 번호</h3>
        <p className="hint">처리와 글자 인식이 끝나면 쪽 번호를 확인합니다</p>
      </section>
    );
  }
  if (!report) {
    return (
      <section className="panel book-panel">
        <h3>쪽 번호</h3>
        <p className="hint">읽는 중</p>
      </section>
    );
  }

  const position = new Map(report.outputs.map((o, i) => [o.outputId, i]));
  const pageLabel = (outputId: string) => `${(position.get(outputId) ?? 0) + 1}번째 장`;
  const realGaps = report.gaps.filter((g) => g.kind === "arabic");
  const lowConfidence = report.outputs.filter((o) => o.number && (o.number.corrected || o.number.confidence < 0.5));
  const problems = report.missing.length + report.duplicates.length + report.outOfOrder.length;

  return (
    <section className="panel book-panel">
      <h3>쪽 번호</h3>
      <p className="book-summary">
        {report.first !== null && report.last !== null ? `${report.first}~${report.last}쪽 | ` : ""}
        {report.total}장 중 {report.detected}장에서 번호를 읽었습니다
      </p>
      {problems === 0 && realGaps.length === 0 && <p className="hint">빠진 쪽, 겹친 쪽이 보이지 않습니다</p>}

      {realGaps.length > 0 && (
        <div className="book-group">
          <h4>빠진 쪽</h4>
          <ul className="book-list">
            {realGaps.map((g) => (
              <li key={`${g.after}-${g.before}`} className={g.likelyUnnumbered ? "maybe" : "missing"}>
                <span>
                  {range(g.after + 1, g.before - 1)}쪽 ({g.missingCount}쪽)
                  {g.unnumberedBetween > 0 && (
                    <span className="hint">
                      {" "}
                      | 사이에 번호 없는 장 {g.unnumberedBetween}장{g.likelyUnnumbered ? ", 빠진 게 아닐 수 있음" : ""}
                    </span>
                  )}
                </span>
              </li>
            ))}
          </ul>
          {report.missing.length > 0 && (
            <p className="hint">iPhone에서 빠진 쪽을 이어 찍으면 문서 끝에 붙습니다. 받은 뒤 아래 정렬을 누르면 제자리로 갑니다.</p>
          )}
        </div>
      )}

      {report.duplicates.length > 0 && (
        <div className="book-group">
          <h4>두 번 찍은 쪽</h4>
          <ul className="book-list">
            {report.duplicates.map((d) => (
              <li key={`${d.kind}-${d.value}`} className="duplicate">
                <span>{d.kind === "roman" ? toRoman(d.value) : d.value}쪽</span>
                <span className="book-jumps">
                  {d.outputIds.map((id) => (
                    <button key={id} className="ghost" onClick={() => onSelect(id)}>
                      {pageLabel(id)}
                    </button>
                  ))}
                </span>
              </li>
            ))}
          </ul>
          <p className="hint">같은 쪽이 두 장 있으면 하나를 골라 "페이지 빼기"로 지웁니다</p>
        </div>
      )}

      {report.outOfOrder.length > 0 && (
        <div className="book-group">
          <h4>순서가 어긋난 장</h4>
          <ul className="book-list">
            {report.outOfOrder.map((id) => {
              const o = report.outputs[position.get(id) ?? 0];
              return (
                <li key={id} className="order">
                  <span>{o.number?.label ?? "?"}쪽</span>
                  <button className="ghost" onClick={() => onSelect(id)}>
                    {pageLabel(id)}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      )}

      <button className="secondary book-sort" disabled={sorting || report.sortMoves === 0} onClick={sort}>
        {sorting ? "정렬하는 중" : report.sortMoves === 0 ? "쪽 번호 순서대로입니다" : `쪽 번호 순서로 정렬 (${report.sortMoves}장 이동)`}
      </button>
      <p className="hint">펼친 면은 원본 사진 한 장 단위로 움직입니다. 번호를 못 읽은 장은 바로 앞 장을 따라갑니다.</p>

      <button className="ghost book-toggle" onClick={() => setShowAll((v) => !v)}>
        {showAll ? "장별 번호 접기" : "장별 번호 보기"}
        {lowConfidence.length > 0 && ` (확인 필요 ${lowConfidence.length})`}
      </button>
      {showAll && (
        <ul className="book-list book-all">
          {report.outputs.map((o, i) => (
            <li key={o.outputId}>
              <button className="ghost book-row" onClick={() => onSelect(o.outputId)}>
                <span className="book-index">{i + 1}</span>
                <span className={o.number ? "book-number" : "book-number none"}>
                  {o.number ? o.number.label : o.hasOcr ? "번호 없음" : "글자 인식 전"}
                </span>
                <span className="book-flags">
                  {o.duplicate && <span className="book-flag duplicate">겹침</span>}
                  {o.outOfOrder && <span className="book-flag order">순서</span>}
                  {o.number?.corrected && (
                    <span className="book-flag corrected">고침 ({o.number.raw})</span>
                  )}
                  {o.number && !o.number.corrected && o.number.confidence < 0.5 && <span className="book-flag low">흐림</span>}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
