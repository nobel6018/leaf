import { useEffect, useRef, useState } from "react";
import { api, fileUrl } from "../api";
import type { PageView, Point, Quad } from "../types";

const CORNERS = ["topLeft", "topRight", "bottomRight", "bottomLeft"] as const;
type Corner = (typeof CORNERS)[number];

const FULL: Quad = {
  topLeft: { x: 0, y: 0 },
  topRight: { x: 1, y: 0 },
  bottomRight: { x: 1, y: 1 },
  bottomLeft: { x: 0, y: 1 },
};

const INSET: Quad = {
  topLeft: { x: 0.05, y: 0.05 },
  topRight: { x: 0.95, y: 0.05 },
  bottomRight: { x: 0.95, y: 0.95 },
  bottomLeft: { x: 0.05, y: 0.95 },
};

interface Props {
  documentId: string;
  page: PageView;
  hasManual: boolean;
  onSave: (quad: Quad | null) => Promise<void>;
  onClose: () => void;
  onError: (e: string) => void;
}

/** 원본 사진 위에서 네 꼭짓점을 끌어 고친다. 좌표는 0~1(원본 회전 적용 후), edits.json quads에 저장한다 */
export function QuadEditor({ documentId, page, hasManual, onSave, onClose, onError }: Props) {
  const [preview, setPreview] = useState<string | null>(null);
  const [quad, setQuad] = useState<Quad>(page.quad ?? INSET);
  const [dragging, setDragging] = useState<Corner | null>(null);
  const [saving, setSaving] = useState(false);
  const imageRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api
      .rawPreview(documentId, page.id)
      .then(setPreview)
      .catch((e) => onError(String(e)));
  }, [documentId, page.id, onError]);

  const toNormalized = (clientX: number, clientY: number): Point => {
    const rect = imageRef.current!.getBoundingClientRect();
    const clamp = (v: number) => Math.min(1, Math.max(0, v));
    return { x: clamp((clientX - rect.left) / rect.width), y: clamp((clientY - rect.top) / rect.height) };
  };

  const save = async (value: Quad | null) => {
    setSaving(true);
    try {
      await onSave(value);
      onClose();
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const points = CORNERS.map((c) => quad[c]);
  const polygon = points.map((p) => `${p.x * 100},${p.y * 100}`).join(" ");

  return (
    <div className="modal" role="dialog" aria-label="꼭짓점 고치기">
      <div className="modal-body quad-editor">
        <div className="modal-head">
          <h2>꼭짓점 고치기</h2>
          <span className="hint">동그라미를 끌어 문서 모서리에 맞추세요</span>
        </div>
        <div className="quad-stage">
          {preview ? (
            <div className="quad-image" ref={imageRef}>
              <img src={fileUrl(preview)} alt="원본 사진" draggable={false} />
              <svg viewBox="0 0 100 100" preserveAspectRatio="none" className="quad-svg">
                <polygon points={polygon} className="quad-area" vectorEffect="non-scaling-stroke" />
              </svg>
              {CORNERS.map((corner) => (
                <div
                  key={corner}
                  className={dragging === corner ? "handle dragging" : "handle"}
                  style={{ left: `${quad[corner].x * 100}%`, top: `${quad[corner].y * 100}%` }}
                  onPointerDown={(e) => {
                    e.currentTarget.setPointerCapture(e.pointerId);
                    setDragging(corner);
                  }}
                  onPointerMove={(e) => {
                    if (dragging !== corner) return;
                    const p = toNormalized(e.clientX, e.clientY);
                    setQuad((q) => ({ ...q, [corner]: p }));
                  }}
                  onPointerUp={() => setDragging(null)}
                  onPointerCancel={() => setDragging(null)}
                />
              ))}
            </div>
          ) : (
            <p className="empty">원본을 여는 중</p>
          )}
        </div>
        <div className="modal-actions">
          <button className="secondary" onClick={() => setQuad(FULL)}>
            사진 전체
          </button>
          {hasManual && (
            <button className="secondary" disabled={saving} onClick={() => save(null)}>
              자동 감지로 되돌리기
            </button>
          )}
          <span className="spacer" />
          <button className="secondary" onClick={onClose}>
            취소
          </button>
          <button className="primary" disabled={saving || !preview} onClick={() => save(quad)}>
            저장하고 다시 처리
          </button>
        </div>
      </div>
    </div>
  );
}
