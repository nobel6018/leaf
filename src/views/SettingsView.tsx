import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { InboxStatus, PairingCode, TransferStatus } from "../types";
import { UpdateAction, UpdateNotes, updateMessage } from "../update/UpdateBanner";
import type { useAppUpdate } from "../update/useAppUpdate";
import { ModelsPanel } from "./ModelsPanel";

interface Props {
  update: ReturnType<typeof useAppUpdate>;
  onError: (e: string) => void;
}

/** iPhone 연결(짝짓기), 연결된 기기, iCloud 받은 편지함, 보정 모델, 앱 버전. @see docs/transfer.md */
export function SettingsView({ update, onError }: Props) {
  const [status, setStatus] = useState<TransferStatus | null | undefined>(undefined);
  const [inbox, setInbox] = useState<InboxStatus | null | undefined>(undefined);
  const [pairing, setPairing] = useState<PairingCode | null>(null);
  const [now, setNow] = useState(Date.now());
  const [paired, setPaired] = useState<string | null>(null);
  const [version, setVersion] = useState("");

  const reload = useCallback(() => {
    api.transferStatus().then(setStatus).catch((e) => onError(String(e)));
    api.icloudInboxStatus().then(setInbox).catch((e) => onError(String(e)));
  }, [onError]);

  useEffect(() => {
    reload();
    getVersion().then(setVersion).catch(() => setVersion(""));
    const unlisten = listen<{ deviceId: string; deviceName: string }>("pairing-completed", (e) => {
      setPairing(null);
      setPaired(e.payload.deviceName);
      reload();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [reload]);

  // 코드 남은 시간을 1초마다 다시 그린다
  useEffect(() => {
    if (!pairing) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [pairing]);

  const remaining = pairing ? Math.max(0, Math.round((new Date(pairing.expiresAt).getTime() - now) / 1000)) : 0;
  useEffect(() => {
    if (pairing && remaining === 0) setPairing(null);
  }, [pairing, remaining]);

  const startPairing = async () => {
    setPaired(null);
    try {
      const code = await api.startPairing();
      if (code === null) onError("iPhone 연결 기능이 아직 연결되지 않았습니다");
      else {
        setNow(Date.now());
        setPairing(code);
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const cancelPairing = () => {
    setPairing(null);
    api.cancelPairing().catch((e) => onError(String(e)));
  };

  const removeDevice = async (deviceId: string, name: string) => {
    if (!window.confirm(`${name} 연결을 끊을까요? 다시 보내려면 새로 짝지어야 합니다.`)) return;
    try {
      await api.removePairedDevice(deviceId);
      reload();
    } catch (e) {
      onError(String(e));
    }
  };

  const updateBusy = update.state.kind === "checking" || update.state.kind === "downloading" || update.state.kind === "installing";

  return (
    <section className="page settings">
      <h1>설정</h1>

      <section className="panel">
        <h3>iPhone 연결</h3>
        {status === null && <p className="hint">전송 기능이 아직 연결되지 않았습니다</p>}
        {status && (
          <p className="hint">
            {status.macName} | {status.listening ? `같은 Wi-Fi에서 받는 중 (포트 ${status.port})` : "받기 꺼짐"}
          </p>
        )}
        {pairing ? (
          <div className="pairing">
            <p>iPhone scanshelf 앱 설정에서 이 코드를 넣으세요</p>
            <div className="pairing-code" aria-label="짝짓기 코드">
              {pairing.code}
            </div>
            <p className="hint">
              {Math.floor(remaining / 60)}분 {String(remaining % 60).padStart(2, "0")}초 남음
            </p>
            <button className="secondary" onClick={cancelPairing}>
              그만두기
            </button>
          </div>
        ) : (
          <button className="primary" disabled={status === null} onClick={startPairing}>
            iPhone 연결
          </button>
        )}
        {paired && <p className="ok-note">{paired}와 연결했습니다</p>}
      </section>

      <section className="panel">
        <h3>연결된 기기</h3>
        {status && status.pairedDevices.length === 0 && <p className="hint">아직 연결한 iPhone이 없습니다</p>}
        <ul className="plain-list">
          {status?.pairedDevices.map((d) => (
            <li key={d.deviceId}>
              <span>
                {d.deviceName}
                <span className="hint"> {new Date(d.pairedAt).toLocaleDateString("ko-KR")} 연결</span>
              </span>
              <button className="ghost" onClick={() => removeDevice(d.deviceId, d.deviceName)}>
                연결 끊기
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section className="panel">
        <h3>iCloud 폴더</h3>
        {inbox === null && <p className="hint">iCloud 받기가 아직 연결되지 않았습니다</p>}
        {inbox && (
          <>
            <p>
              {inbox.available ? "iCloud Drive를 지켜보는 중" : "iCloud Drive를 찾지 못했습니다. 시스템 설정에서 iCloud Drive를 켜세요."}
            </p>
            <p className="hint path">{inbox.path}</p>
            {inbox.pending > 0 && <p className="hint">아직 다 받지 못한 문서 {inbox.pending}개</p>}
          </>
        )}
      </section>

      <ModelsPanel onError={onError} />

      <section className="panel">
        <h3>앱 버전</h3>
        <div className="setting-row">
          <span>
            scanshelf {version}
            {update.state.kind !== "none" && <span className="hint"> {updateMessage(update)}</span>}
            <UpdateNotes update={update} />
          </span>
          <span className="button-row">
            <UpdateAction update={update} onError={onError} />
            <button className="secondary" disabled={updateBusy} onClick={update.checkNow}>
              업데이트 확인하기
            </button>
          </span>
        </div>
      </section>
    </section>
  );
}
