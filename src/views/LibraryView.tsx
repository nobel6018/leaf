import { useMemo, useState } from "react";
import { api, fileUrl, KIND_LABEL } from "../api";
import type { DocumentState, DocumentSummary, Progress } from "../types";

const NO_SESSION = "세션 없음";

export function stateLabel(state: DocumentState, progress?: Progress): { text: string; tone: string } {
  if (progress && progress.stage !== "done") {
    const stage = progress.stage === "ocr" ? "글자 읽는 중" : "보정 중";
    return { text: `${stage} ${progress.done + 1}/${progress.total}`, tone: "busy" };
  }
  switch (state.status) {
    case "unprocessed":
      return { text: "처리 전", tone: "muted" };
    case "queued":
      return { text: "처리 대기", tone: "busy" };
    case "processing":
      return { text: `처리 중 ${state.done}/${state.total}`, tone: "busy" };
    case "stale":
      return { text: `다시 처리할 페이지 ${state.pages}장`, tone: "warn" };
    case "failed":
      return { text: "처리 실패", tone: "error" };
    case "ready":
      return { text: "완료", tone: "ok" };
  }
}

interface Props {
  documents: DocumentSummary[];
  progress: Record<string, Progress>;
  onOpen: (id: string) => void;
  onError: (e: string) => void;
}

export function LibraryView({ documents, progress, onOpen, onError }: Props) {
  const [csvBusy, setCsvBusy] = useState<string | null>(null);

  // 세션은 최근 문서가 있는 순서대로. 문서는 이미 최신순으로 온다
  const groups = useMemo(() => {
    const map = new Map<string, DocumentSummary[]>();
    for (const doc of documents) {
      const key = doc.session?.trim() || NO_SESSION;
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(doc);
    }
    return [...map.entries()];
  }, [documents]);

  const exportCsv = async (session: string) => {
    setCsvBusy(session);
    try {
      const result = await api.exportNotionCsv(session);
      if (result === null) onError("노션 CSV 내보내기는 아직 연결되지 않았습니다");
    } catch (e) {
      onError(String(e));
    } finally {
      setCsvBusy(null);
    }
  };

  if (documents.length === 0) {
    return (
      <section className="page">
        <h1>보관함</h1>
        <p className="empty">iPhone에서 찍은 문서가 여기에 모입니다. 설정에서 iPhone을 연결하세요.</p>
      </section>
    );
  }

  return (
    <section className="page">
      <h1>보관함</h1>
      {groups.map(([session, docs]) => {
        const pamphlets = docs.filter((d) => d.kind === "pamphlet").length;
        return (
          <div className="session" key={session}>
            <div className="session-head">
              <h2>
                {session} <span className="count">{docs.length}개</span>
              </h2>
              {session !== NO_SESSION && pamphlets > 0 && (
                <button className="secondary" disabled={csvBusy === session} onClick={() => exportCsv(session)}>
                  {csvBusy === session ? "내보내는 중" : "노션 CSV 내보내기"}
                </button>
              )}
            </div>
            <div className="doc-grid">
              {docs.map((doc) => {
                const label = stateLabel(doc.state, progress[doc.id]);
                return (
                  <button className="doc-card" key={doc.id} onClick={() => onOpen(doc.id)}>
                    <div className="cover">
                      {doc.cover ? (
                        <img src={fileUrl(doc.cover, doc.state.status)} alt="" loading="lazy" />
                      ) : (
                        <span className="cover-empty">{doc.pageCount}장</span>
                      )}
                    </div>
                    <div className="doc-meta">
                      <span className={`badge kind-${doc.kind}`}>{KIND_LABEL[doc.kind]}</span>
                      <span className={`badge state-${label.tone}`}>{label.text}</span>
                    </div>
                    <div className="doc-name">{doc.name}</div>
                    <div className="doc-sub">
                      {doc.pageCount}장 | {new Date(doc.createdAt).toLocaleDateString("ko-KR")}
                      {doc.hasSummary && " | AI 정리"}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        );
      })}
    </section>
  );
}
