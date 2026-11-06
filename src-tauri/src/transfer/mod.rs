//! iPhone에서 Mac으로 보내기(Wi-Fi 직접 전송). 앱을 켜면 LAN에 HTTP 서버를 열고 Bonjour로 알린다.
//!
//! 화면 쪽에는 Tauri 명령(`transfer_status`, `start_pairing`, `cancel_pairing`, `remove_paired_device`)과
//! 이벤트(`library-changed`, `pairing-completed`)로만 드러난다.
//!
//! @see docs/transfer.md

pub mod bonjour;
pub mod devices;
pub mod pairing;
pub mod server;
pub mod sessions;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::store::Store;
use bonjour::Advertiser;
use devices::{Devices, KeychainTokenStore, PairedDevice};
use pairing::{Pairing, PairingCode};
use server::{TransferEvents, TransferService};

pub const EVENT_LIBRARY_CHANGED: &str = "library-changed";
pub const EVENT_PAIRING_COMPLETED: &str = "pairing-completed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub document_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCompleted {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatus {
    pub listening: bool,
    pub port: u16,
    pub mac_name: String,
    pub paired_devices: Vec<PairedDevice>,
}

pub struct TransferState {
    service: Arc<TransferService>,
    port: u16,
    listening: Arc<AtomicBool>,
    _advertiser: Mutex<Option<Advertiser>>,
}

/// 보관함이 바뀌었다고 화면에 알린다. Wi-Fi 전송과 iCloud inbox가 같이 쓴다
pub fn emit_library_changed(app: &AppHandle, document_id: Uuid) {
    let payload = LibraryChanged {
        document_id: document_id.to_string(),
    };
    if let Err(error) = app.emit(EVENT_LIBRARY_CHANGED, payload) {
        eprintln!("library-changed 이벤트를 보내지 못했다: {error}");
    }
}

struct TauriEvents(AppHandle);

impl TransferEvents for TauriEvents {
    fn library_changed(&self, document_id: Uuid) {
        emit_library_changed(&self.0, document_id);
    }

    fn pairing_completed(&self, device: &PairedDevice) {
        let payload = PairingCompleted {
            device_id: device.device_id.clone(),
            device_name: device.device_name.clone(),
        };
        if let Err(error) = self.0.emit(EVENT_PAIRING_COMPLETED, payload) {
            eprintln!("pairing-completed 이벤트를 보내지 못했다: {error}");
        }
    }
}

/// 서버를 열고 Bonjour로 알린 뒤 상태를 Tauri에 맡긴다. 포트를 못 열어도 앱은 뜨고 `listening: false`로 보인다
pub fn setup(app: &AppHandle, store: Arc<Store>, app_data_dir: &Path) {
    let mac_name = mac_name();
    let devices = Arc::new(Devices::load(&app_data_dir.join("devices.json"), Arc::new(KeychainTokenStore)));
    let service = Arc::new(TransferService {
        store,
        devices,
        pairing: Arc::new(Pairing::default()),
        mac_name: mac_name.clone(),
        events: Arc::new(TauriEvents(app.clone())),
        uploads: Default::default(),
    });

    let listening = Arc::new(AtomicBool::new(false));
    // 포트는 OS가 고른다. iPhone은 Bonjour로 포트를 알아내므로 고정할 필요가 없다
    let bound = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .and_then(|listener| listener.set_nonblocking(true).map(|_| listener));
    let (port, advertiser) = match bound {
        Ok(listener) => {
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            listening.store(true, Ordering::SeqCst);
            let running = listening.clone();
            let server_service = service.clone();
            tauri::async_runtime::spawn(async move {
                let result = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => server::serve(listener, server_service).await,
                    Err(error) => Err(error),
                };
                running.store(false, Ordering::SeqCst);
                if let Err(error) = result {
                    eprintln!("전송 서버가 멈췄다: {error}");
                }
            });
            eprintln!("전송 서버 시작: 포트 {port}");
            let advertiser = match Advertiser::start(&mac_name, port) {
                Ok(advertiser) => Some(advertiser),
                Err(error) => {
                    eprintln!("{error}");
                    None
                }
            };
            (port, advertiser)
        }
        Err(error) => {
            eprintln!("전송 서버 포트를 열지 못했다: {error}");
            (0, None)
        }
    };

    app.manage(TransferState {
        service,
        port,
        listening,
        _advertiser: Mutex::new(advertiser),
    });
}

/// Bonjour 이름과 `info`에 쓰는 Mac 이름. 시스템 설정의 "컴퓨터 이름"
pub fn mac_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("scutil").args(["--get", "ComputerName"]).output() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if output.status.success() && !name.is_empty() {
                return name;
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(name) = std::env::var("COMPUTERNAME") {
            if !name.is_empty() {
                return name;
            }
        }
    }
    "scanshelf".to_string()
}

#[tauri::command]
pub fn transfer_status(state: tauri::State<'_, TransferState>) -> TransferStatus {
    TransferStatus {
        listening: state.listening.load(Ordering::SeqCst),
        port: state.port,
        mac_name: state.service.mac_name.clone(),
        paired_devices: state.service.devices.list(),
    }
}

#[tauri::command]
pub fn start_pairing(state: tauri::State<'_, TransferState>) -> PairingCode {
    state.service.pairing.start(Utc::now())
}

#[tauri::command]
pub fn cancel_pairing(state: tauri::State<'_, TransferState>) {
    state.service.pairing.cancel();
}

#[tauri::command]
pub fn remove_paired_device(state: tauri::State<'_, TransferState>, device_id: String) -> Result<(), String> {
    state.service.devices.remove(&device_id)
}
