import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { PamphletSummary, Relevance } from "../types";

type TextField = Exclude<
  keyof PamphletSummary,
  "relevance" | "editedFields" | "generatedAt" | "generatedBy" | "sourceFile"
>;

// 노션 CSV 열 순서(docs/features.md)
const FIELDS: { key: TextField; label: string; multiline?: boolean }[] = [
  { key: "company", label: "회사명" },
  { key: "country", label: "국가" },
  { key: "event", label: "박람회명" },
  { key: "date", label: "날짜" },
  { key: "products", label: "주력 제품과 기술", multiline: true },
  { key: "keySpecs", label: "주요 스펙", multiline: true },
  { key: "targetMarket", label: "타깃 시장과 고객", multiline: true },
  { key: "references", label: "레퍼런스", multiline: true },
  { key: "relevanceNote", label: "관련성 근거", multiline: true },
  { key: "contact", label: "연락처", multiline: true },
];

const RELEVANCE: { value: Relevance; label: string }[] = [
  { value: "competitor", label: "경쟁사" },
  { value: "partnerCandidate", label: "협력 후보" },
  { value: "supplier", label: "공급사" },
  { value: "unrelated", label: "관련 없음" },
];

interface Props {
  documentId: string;
  ready: boolean;
  onRename: (name: string) => void;
  onError: (e: string) => void;
}

/** 팜플렛 AI 정리(summary.json). 고친 칸은 editedFields에 남아 다시 정리해도 덮이지 않는다 */
export function AiPanel({ documentId, ready, onRename, onError }: Props) {
  const [summary, setSummary] = useState<PamphletSummary | null>(null);
  const [draft, setDraft] = useState<Partial<Record<TextField, string>>>({});
  const [busy, setBusy] = useState<"summarize" | "name" | null>(null);

  const load = useCallback(() => {
    api
      .getSummary(documentId)
      .then((s) => {
        setSummary(s);
        setDraft({});
      })
      .catch((e) => onError(String(e)));
  }, [documentId, onError]);

  useEffect(load, [load]);

  // 처리 직후 자동 정리(팜플렛)가 끝나면 다시 읽는다(src-tauri/src/ai.rs summarize_if_pamphlet)
  useEffect(() => {
    const unlisten = listen<{ documentId: string }>("summary-changed", (e) => {
      if (e.payload.documentId === documentId) load();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [documentId, load]);

  const saveField = async (field: string, value: unknown) => {
    try {
      setSummary(await api.saveSummaryField(documentId, field, value));
    } catch (e) {
      onError(String(e));
    }
  };

  const summarize = async () => {
    setBusy("summarize");
    try {
      const result = await api.summarizePamphlet(documentId);
      if (result === null) onError("AI 정리는 아직 연결되지 않았습니다");
      else setSummary(result);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const suggestName = async () => {
    setBusy("name");
    try {
      const result = await api.suggestDocumentName(documentId);
      if (result === null) {
        onError("이름 추천은 아직 연결되지 않았거나 추천할 이름이 없습니다");
        return;
      }
      const name = typeof result === "string" ? result : result.name;
      if (name && window.confirm(`문서 이름을 "${name}"(으)로 바꿀까요?`)) onRename(name);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="panel">
      <h3>AI 정리</h3>
      <div className="button-row">
        <button className="secondary" disabled={!ready || busy !== null} onClick={summarize}>
          {busy === "summarize" ? "정리하는 중" : summary ? "다시 정리" : "AI 정리"}
        </button>
        <button className="secondary" disabled={!ready || busy !== null} onClick={suggestName}>
          {busy === "name" ? "추천 받는 중" : "이름 추천"}
        </button>
      </div>
      {!summary && <p className="hint">OCR이 끝난 뒤 AI 정리를 누르면 노션 CSV 열에 맞춰 정리합니다</p>}
      {summary && (
        <div className="summary-form">
          {FIELDS.map(({ key, label, multiline }) => {
            const value = draft[key] ?? summary[key];
            const edited = summary.editedFields.includes(key);
            const commit = () => {
              if (draft[key] !== undefined && draft[key] !== summary[key]) saveField(key, draft[key]);
            };
            return (
              <label key={key} className="field">
                <span>
                  {label}
                  {edited && <em className="edited"> 고침</em>}
                </span>
                {multiline ? (
                  <textarea
                    rows={2}
                    value={value}
                    onChange={(e) => setDraft((d) => ({ ...d, [key]: e.target.value }))}
                    onBlur={commit}
                  />
                ) : (
                  <input
                    value={value}
                    onChange={(e) => setDraft((d) => ({ ...d, [key]: e.target.value }))}
                    onBlur={commit}
                    onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                  />
                )}
              </label>
            );
          })}
          <div className="field">
            <span>
              관련성{summary.editedFields.includes("relevance") && <em className="edited"> 고침</em>}
            </span>
            <div className="chips">
              {RELEVANCE.map(({ value, label }) => {
                const on = summary.relevance.includes(value);
                return (
                  <button
                    key={value}
                    className={on ? "chip on" : "chip"}
                    aria-pressed={on}
                    onClick={() =>
                      saveField(
                        "relevance",
                        on ? summary.relevance.filter((r) => r !== value) : [...summary.relevance, value],
                      )
                    }
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </div>
          <p className="hint">
            {summary.generatedBy} | {new Date(summary.generatedAt).toLocaleString("ko-KR")}
          </p>
        </div>
      )}
    </section>
  );
}
