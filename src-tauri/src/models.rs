//! 곡면 보정(UVDoc), 인페인팅(LaMa) 모델 관리. 설치 파일에 넣지 않고 처음 쓸 때 Release 자산에서 받는다.
//! 모델이 없어도 처리는 된다: 곡면 보정은 건너뛰고, 손가락과 필기 자리는 종이 색으로 채운다.
//!
//! @see docs/models.md

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use leaf_vision::models::{MODELS, verify};
use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::download::{self, Item};

static INSTALLING: Mutex<bool> = Mutex::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn models_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app.path().app_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했다: {e}"))?;
    Ok(base.join("models"))
}

/// 처리 단계가 쓰는 모델 묶음. 확인을 통과한 파일만 넘긴다(받다 만 파일을 ONNX Runtime이 읽지 않게)
pub fn load(app: &tauri::AppHandle) -> leaf_vision::Models {
    match models_dir(app) {
        Ok(dir) => leaf_vision::Models::new(dir),
        Err(_) => leaf_vision::Models::none(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub file: &'static str,
    pub purpose: &'static str,
    pub size: u64,
    pub license: &'static str,
    pub installed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsStatus {
    pub models: Vec<ModelStatus>,
    pub installing: bool,
}

fn purpose(file: &str) -> &'static str {
    if file.starts_with("uvdoc") { "곡면 보정" } else { "손가락, 필기 지우기" }
}

#[tauri::command]
pub fn models_status(app: tauri::AppHandle) -> Result<ModelsStatus, String> {
    let dir = models_dir(&app)?;
    let models = MODELS
        .iter()
        .map(|spec| ModelStatus {
            file: spec.file,
            purpose: purpose(spec.file),
            size: spec.size,
            license: spec.license,
            // 크기만 본다. 해시 확인은 받은 직후 한 번 한다(200MB 해시를 화면 열 때마다 하지 않게)
            installed: std::fs::metadata(dir.join(spec.file)).is_ok_and(|m| m.len() == spec.size),
        })
        .collect();
    Ok(ModelsStatus {
        models,
        installing: *INSTALLING.lock().unwrap_or_else(|e| e.into_inner()),
    })
}

/// 모델을 받는다. 진행은 `model-install-progress`, 끝은 `model-install-finished { error }` 이벤트
#[tauri::command]
pub fn install_models(app: tauri::AppHandle) -> Result<(), String> {
    {
        let mut installing = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
        if *installing {
            return Ok(());
        }
        *installing = true;
    }
    CANCEL.store(false, Ordering::SeqCst);
    let dir = models_dir(&app)?;
    std::thread::spawn(move || {
        let _busy = crate::busy::start("모델 내려받기");
        let result = install(&app, &dir);
        *INSTALLING.lock().unwrap_or_else(|e| e.into_inner()) = false;
        let _ = app.emit("model-install-finished", serde_json::json!({ "error": result.err() }));
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_model_install() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn install(app: &tauri::AppHandle, dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{} 만들기 실패: {e}", dir.display()))?;
    let items: Vec<Item> = MODELS
        .iter()
        .map(|spec| Item {
            name: spec.file.to_string(),
            url: spec.url.to_string(),
            dest: dir.join(spec.file),
            size: spec.size,
        })
        .collect();
    download::download_all(&items, &CANCEL, |progress| {
        let _ = app.emit("model-install-progress", progress);
    })
    .map_err(|e| format!("모델을 받지 못했습니다: {e}"))?;
    for spec in MODELS {
        let path = dir.join(spec.file);
        if let Err(e) = verify(&path, spec) {
            // 깨진 파일은 지워 다음에 처음부터 받게 한다
            let _ = std::fs::remove_file(&path);
            return Err(format!("{} 확인 실패: {e}", spec.file));
        }
    }
    Ok(())
}
