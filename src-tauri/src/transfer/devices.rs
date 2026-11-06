//! 짝지은 iPhone 목록과 기기별 토큰.
//!
//! 토큰은 OS 키체인(macOS Keychain, Windows 자격 증명 관리자)에만 두고, 앱 데이터의 `devices.json`에는
//! 기기 id, 이름, 짝지은 시각만 둔다. 토큰은 어떤 로그에도 남기지 않는다.
//!
//! @see docs/transfer.md

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 키체인 항목의 서비스 이름. 계정 이름은 기기 id
const KEYCHAIN_SERVICE: &str = "com.nobel6018.scanshelf.transfer";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub device_id: String,
    pub device_name: String,
    pub paired_at: DateTime<Utc>,
}

/// 토큰 저장소. 앱은 키체인을 쓰고, 테스트는 메모리를 쓴다
pub trait TokenStore: Send + Sync {
    fn get(&self, device_id: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, device_id: &str, token: &[u8]) -> Result<(), String>;
    fn delete(&self, device_id: &str) -> Result<(), String>;
}

pub struct KeychainTokenStore;

impl TokenStore for KeychainTokenStore {
    fn get(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, device_id).map_err(|e| format!("키체인을 열지 못했다: {e}"))?;
        match entry.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("키체인에서 토큰을 읽지 못했다: {e}")),
        }
    }

    fn set(&self, device_id: &str, token: &[u8]) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, device_id).map_err(|e| format!("키체인을 열지 못했다: {e}"))?;
        entry
            .set_secret(token)
            .map_err(|e| format!("키체인에 토큰을 쓰지 못했다: {e}"))
    }

    fn delete(&self, device_id: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, device_id).map_err(|e| format!("키체인을 열지 못했다: {e}"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("키체인에서 토큰을 지우지 못했다: {e}")),
        }
    }
}

#[derive(Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<HashMap<String, Vec<u8>>>,
}

impl TokenStore for MemoryTokenStore {
    fn get(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.tokens.lock().unwrap_or_else(|e| e.into_inner()).get(device_id).cloned())
    }

    fn set(&self, device_id: &str, token: &[u8]) -> Result<(), String> {
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(device_id.to_string(), token.to_vec());
        Ok(())
    }

    fn delete(&self, device_id: &str) -> Result<(), String> {
        self.tokens.lock().unwrap_or_else(|e| e.into_inner()).remove(device_id);
        Ok(())
    }
}

pub struct Devices {
    file: PathBuf,
    tokens: Arc<dyn TokenStore>,
    list: Mutex<Vec<PairedDevice>>,
    /// 요청마다 키체인을 열면 느리고, 서명되지 않은 앱에서는 접근 확인 창이 뜰 수 있어 한 번 읽은 토큰을 기억한다
    cache: Mutex<HashMap<String, Vec<u8>>>,
}

impl Devices {
    pub fn load(file: &Path, tokens: Arc<dyn TokenStore>) -> Devices {
        let list = match fs::read(file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                eprintln!("devices.json을 읽지 못해 빈 목록으로 시작: {error}");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        Devices {
            file: file.to_path_buf(),
            tokens,
            list: Mutex::new(list),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Vec<PairedDevice> {
        self.list.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 같은 기기가 다시 짝지으면 토큰과 이름을 새로 바꾼다
    pub fn add(&self, device_id: &str, device_name: &str, token: &[u8]) -> Result<PairedDevice, String> {
        self.tokens.set(device_id, token)?;
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(device_id.to_string(), token.to_vec());
        let device = PairedDevice {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            paired_at: Utc::now(),
        };
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        list.retain(|d| d.device_id != device_id);
        list.push(device.clone());
        save_list(&self.file, &list)?;
        Ok(device)
    }

    pub fn remove(&self, device_id: &str) -> Result<(), String> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).remove(device_id);
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        list.retain(|d| d.device_id != device_id);
        save_list(&self.file, &list)?;
        drop(list);
        self.tokens.delete(device_id)
    }

    /// 목록에 있는 기기의 토큰만 돌려준다. 목록에서 지운 기기는 키체인에 토큰이 남아 있어도 거부한다
    pub fn token(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        let known = self
            .list
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|d| d.device_id == device_id);
        if !known {
            return Ok(None);
        }
        if let Some(token) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(device_id) {
            return Ok(Some(token.clone()));
        }
        let token = self.tokens.get(device_id)?;
        if let Some(token) = &token {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(device_id.to_string(), token.clone());
        }
        Ok(token)
    }
}

fn save_list(file: &Path, list: &[PairedDevice]) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("앱 데이터 폴더를 만들지 못했다: {e}"))?;
    }
    let json = serde_json::to_vec_pretty(list).expect("기기 목록 직렬화는 실패하지 않는다");
    let temp = file.with_extension("json.tmp");
    fs::write(&temp, json)
        .and_then(|_| fs::rename(&temp, file))
        .map_err(|e| format!("devices.json을 저장하지 못했다: {e}"))
}
