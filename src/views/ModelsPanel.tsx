import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// 곡면 보정, 손가락과 필기 지우기 모델. 설치 파일에 넣지 않고 여기서 받는다. @see docs/models.md

interface ModelStatus {
  file: string;
  purpose: string;
  size: number;
  license: string;
  installed: boolean;
}

interface ModelsStatus {
  models: ModelStatus[];
  installing: boolean;
}

interface Progress {
  file: string;
  received: number;
  total: number;
}

const mb = (bytes: number) => `${Math.round(bytes / 1024 / 1024)}MB`;

export function ModelsPanel({ onError }: { onError: (e: string) => void }) {
  const [status, setStatus] = useState<ModelsStatus | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);

  const reload = useCallback(() => {
    invoke<ModelsStatus>("models_status").then(setStatus).catch((e) => onError(String(e)));
  }, [onError]);

  useEffect(() => {
    reload();
    const progressListener = listen<Progress>("model-install-progress", (e) => setProgress(e.payload));
    const finishedListener = listen<{ error: string | null }>("model-install-finished", (e) => {
      setProgress(null);
      if (e.payload.error) onError(e.payload.error);
      reload();
    });
    return () => {
      progressListener.then((f) => f());
      finishedListener.then((f) => f());
    };
  }, [reload, onError]);

  if (!status) return null;
  const missing = status.models.filter((m) => !m.installed);
  const installing = status.installing || progress !== null;
  const total = status.models.reduce((sum, m) => sum + m.size, 0);

  return (
    <section className="panel">
      <h3>보정 모델</h3>
      <p className="hint">
        모델이 없어도 스캔과 OCR은 됩니다. 받으면 휘어진 책장을 펴고, 손가락과 색 펜 자국을 더 자연스럽게 지웁니다.
      </p>
      <ul className="model-list">
        {status.models.map((m) => (
          <li key={m.file}>
            <span>{m.purpose}</span>
            <span className="hint">
              {mb(m.size)}, {m.license} {m.installed ? "설치됨" : "없음"}
            </span>
          </li>
        ))}
      </ul>
      <div className="setting-row">
        <span className="hint">
          {installing && progress
            ? `받는 중 ${Math.floor((progress.received / progress.total) * 100)}% (${mb(progress.received)} / ${mb(progress.total)})`
            : missing.length === 0
              ? "모두 설치됨"
              : `받을 크기 ${mb(total)}. 끊겨도 이어서 받습니다`}
        </span>
        <span className="button-row">
          {installing ? (
            <button className="secondary" onClick={() => invoke("cancel_model_install").catch((e) => onError(String(e)))}>
              멈추기
            </button>
          ) : (
            missing.length > 0 && (
              <button className="primary" onClick={() => invoke("install_models").then(reload).catch((e) => onError(String(e)))}>
                모델 받기
              </button>
            )
          )}
        </span>
      </div>
    </section>
  );
}
