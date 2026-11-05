import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { DocumentSummary, Progress } from "./types";
import { UpdateBanner } from "./update/UpdateBanner";
import { useAppUpdate } from "./update/useAppUpdate";
import { LibraryView } from "./views/LibraryView";
import { DocumentView } from "./views/DocumentView";
import { SearchView } from "./views/SearchView";
import { SettingsView } from "./views/SettingsView";
import { AskWindow } from "./views/AskWindow";

export type Route =
  | { view: "library" }
  | { view: "document"; id: string; outputId?: string; query?: string }
  | { view: "search" }
  | { view: "settings" };

export default function App() {
  const [route, setRoute] = useState<Route>({ view: "library" });
  const [documents, setDocuments] = useState<DocumentSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  // 처리 진행은 이벤트로 오고, 문서 화면도 같은 값을 본다
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  // 문서가 바뀌면 열려 있는 문서 화면이 다시 읽도록 올린다
  const [revision, setRevision] = useState(0);
  const update = useAppUpdate();
  const refreshTimer = useRef<number | null>(null);

  const refresh = useCallback(() => {
    api
      .listDocuments()
      .then((docs) => {
        setDocuments(docs);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  // 여러 이벤트가 몰려도 한 번만 다시 읽는다
  const scheduleRefresh = useCallback(() => {
    if (refreshTimer.current !== null) return;
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = null;
      refresh();
      setRevision((r) => r + 1);
    }, 250);
  }, [refresh]);

  useEffect(() => {
    refresh();
    const unlisten = [
      listen<Progress>("processing-progress", (e) => {
        setProgress((p) => ({ ...p, [e.payload.documentId]: e.payload }));
        if (e.payload.stage === "decode" || e.payload.stage === "done") scheduleRefresh();
      }),
      listen<{ documentId: string; error: string | null }>("document-processed", (e) => {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.payload.documentId];
          return next;
        });
        if (e.payload.error) setError(`처리 실패: ${e.payload.error}`);
        scheduleRefresh();
      }),
      listen("library-changed", scheduleRefresh),
    ];
    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
    };
  }, [refresh, scheduleRefresh]);

  const nav = (target: Route["view"], label: string) => (
    <button
      className={route.view === target || (target === "library" && route.view === "document") ? "nav active" : "nav"}
      onClick={() => setRoute({ view: target } as Route)}
    >
      {label}
    </button>
  );

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">leaf</div>
        {nav("library", "보관함")}
        {nav("search", "검색")}
        {nav("settings", "설정")}
      </aside>
      <main className="content">
        {update.state.kind !== "none" && <UpdateBanner update={update} onError={setError} />}
        {error && (
          <div className="error-bar" role="alert">
            <span>{error}</span>
            <button className="ghost" onClick={() => setError(null)}>
              닫기
            </button>
          </div>
        )}
        {route.view === "library" && (
          <LibraryView
            documents={documents}
            progress={progress}
            onOpen={(id) => setRoute({ view: "document", id })}
            onError={setError}
          />
        )}
        {route.view === "document" && (
          <DocumentView
            key={route.id}
            documentId={route.id}
            initialOutputId={route.outputId}
            initialQuery={route.query}
            revision={revision}
            progress={progress[route.id]}
            onBack={() => setRoute({ view: "library" })}
            onChanged={scheduleRefresh}
            onError={setError}
          />
        )}
        {route.view === "search" && (
          <SearchView
            onOpen={(id, outputId, query) => setRoute({ view: "document", id, outputId, query })}
            onError={setError}
          />
        )}
        {route.view === "settings" && <SettingsView update={update} onError={setError} />}
      </main>
      {/* ⌘J 모든 문서에 질문(docs/ask.md). 다른 곳에서는 openAsk()로 연다 */}
      <AskWindow documents={documents} onOpen={(id, outputId) => setRoute({ view: "document", id, outputId: outputId ?? undefined })} />
    </div>
  );
}
