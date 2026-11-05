import { Menu } from "@tauri-apps/api/menu";
import { api } from "./api";
import type { DocumentSummary, ExportFormat } from "./types";

// 보관함 문서 카드의 우클릭 메뉴. 웹뷰 기본 메뉴(다시 불러오기) 대신 OS 메뉴를 띄운다
// @see docs/context-menu.md

export interface DocumentMenuContext {
  open: (doc: DocumentSummary) => void;
  /** 다시 처리, 지우기 뒤 목록을 다시 읽는다 */
  refresh: () => void;
  onDeleted: (id: string) => void;
  /** 내보내기 결과처럼 알릴 것. path가 있으면 Finder에서 보기 단추를 같이 띄운다 */
  onNotice: (text: string, path?: string) => void;
  onError: (message: string) => void;
}

/** 처리 대기나 처리 중인 문서. 다시 처리와 지우기를 막는다 */
export function isBusy(doc: DocumentSummary): boolean {
  return doc.state.status === "queued" || doc.state.status === "processing";
}

/** 처리된 페이지가 있어 내보낼 수 있는지. 다시 처리할 페이지가 남아 있어도 지난 결과로 내보낸다 */
export function hasOutputs(doc: DocumentSummary): boolean {
  return doc.cover !== null || doc.state.status === "ready" || doc.state.status === "stale";
}

const EXPORTS: { format: ExportFormat; label: string }[] = [
  { format: "pdf", label: "PDF" },
  { format: "jpg", label: "JPG 묶음" },
  { format: "txt", label: "텍스트" },
];

/** 휴지통으로 옮기기 전에 묻는다. 단축키(⌘⌫)와 메뉴가 같은 문구를 쓴다 */
export async function confirmAndDelete(doc: { id: string; name: string }, ctx: Pick<DocumentMenuContext, "onDeleted" | "onError">) {
  if (!confirm(`"${doc.name}" 문서를 휴지통으로 옮길까요?\nFinder 휴지통에서 되살릴 수 있습니다.`)) return;
  try {
    await api.deleteDocument(doc.id);
    ctx.onDeleted(doc.id);
  } catch (e) {
    ctx.onError(`${doc.name}: ${e}`);
  }
}

export async function showDocumentMenu(doc: DocumentSummary, ctx: DocumentMenuContext): Promise<void> {
  const busy = isBusy(doc);
  const ready = hasOutputs(doc);
  const fail = (e: unknown) => ctx.onError(`${doc.name}: ${e}`);

  const exportAs = (label: string, task: () => Promise<unknown>) => async () => {
    try {
      const result = await task();
      if (result === null) return ctx.onError(`${label} 내보내기는 아직 연결되지 않았습니다`);
      const path = typeof result === "string" ? result : (result as { path?: string } | undefined)?.path;
      ctx.onNotice(`${doc.name}: ${label}로 내보냈습니다`, path);
    } catch (e) {
      fail(e);
    }
  };

  const menu = await Menu.new({
    items: [
      { text: "열기", action: () => ctx.open(doc) },
      {
        text: "다시 처리",
        enabled: !busy,
        action: () =>
          api
            .reprocess(doc.id, true)
            .then(ctx.refresh)
            .catch(fail),
      },
      { item: "Separator" },
      {
        text: "내보내기",
        enabled: ready,
        items: [
          ...EXPORTS.map(({ format, label }) => ({
            text: label,
            action: exportAs(label, () => api.exportDocument(doc.id, format)),
          })),
          { text: "Word", action: exportAs("Word", () => api.exportDocx(doc.id)) },
          { text: "AI 묶음", action: exportAs("AI 묶음", () => api.exportAiBundle(doc.id)) },
        ],
      },
      {
        text: "글자를 마크다운으로 복사",
        enabled: ready,
        action: async () => {
          try {
            await navigator.clipboard.writeText(await api.documentMarkdown(doc.id));
            ctx.onNotice(`${doc.name}: 마크다운을 복사했습니다`);
          } catch (e) {
            fail(e);
          }
        },
      },
      { text: "Finder에서 보기", action: () => api.revealDocument(doc.id).catch(fail) },
      { item: "Separator" },
      {
        text: busy ? "휴지통으로 옮기기 (처리 중)" : "휴지통으로 옮기기…",
        enabled: !busy,
        action: () => confirmAndDelete(doc, ctx),
      },
    ],
  });
  await menu.popup();
}
