import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DocumentKind, DocumentSummary } from "./types";
import { UpdateBanner } from "./update/UpdateBanner";
import { useAppUpdate } from "./update/useAppUpdate";

const KIND_LABEL: Record<DocumentKind, string> = {
  pamphlet: "팜플렛",
  book: "책",
  document: "문서",
};

export default function App() {
  const [documents, setDocuments] = useState<DocumentSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const update = useAppUpdate();

  useEffect(() => {
    invoke<DocumentSummary[]>("list_documents")
      .then(setDocuments)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="library">
      {update.state.kind !== "none" && <UpdateBanner update={update} onError={setError} />}
      <h1>보관함</h1>
      {error && <p className="error">{error}</p>}
      {documents.length === 0 && !error && (
        <p className="empty">iPhone에서 찍은 문서가 여기에 모입니다.</p>
      )}
      <ul>
        {documents.map((doc) => (
          <li key={doc.id}>
            <span className="kind">{KIND_LABEL[doc.kind]}</span>
            <span className="name">{doc.name}</span>
            <span className="pages">{doc.pageCount}장</span>
          </li>
        ))}
      </ul>
    </main>
  );
}
