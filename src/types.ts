// src-tauri/src/library.rs DocumentSummary와 같은 모양
export type DocumentKind = "pamphlet" | "book" | "document";

export interface DocumentSummary {
  id: string;
  name: string;
  kind: DocumentKind;
  session: string | null;
  pageCount: number;
  createdAt: string;
}
