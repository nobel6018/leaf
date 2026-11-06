//! 처리 작업 줄. 사진 한 장이 수백 MB 메모리를 쓰므로 문서를 하나씩 처리하는 작업 스레드 하나를 둔다.
//!
//! 화면에는 `processing-progress`, `document-processed` 이벤트로 알린다.
//!
//! @see docs/pipeline.md

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use serde::Serialize;
use tauri::{Emitter, Manager};

use super::{ProcessContext, Progress, process_document};
use crate::library::{cache_dir, library_dir};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running { done: usize, total: usize },
    Failed { message: String },
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<(String, bool)>,
    states: HashMap<String, JobState>,
    running: Option<String>,
}

#[derive(Default)]
struct Inner {
    queue: Mutex<Queue>,
    wake: Condvar,
    io_lock: Mutex<()>,
}

#[derive(Clone, Default)]
pub struct Pipeline {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finished {
    pub document_id: String,
    pub error: Option<String>,
}

impl Pipeline {
    pub fn job_state(&self, document_id: &str) -> Option<JobState> {
        self.lock().states.get(document_id).cloned()
    }

    pub fn states(&self) -> HashMap<String, JobState> {
        self.lock().states.clone()
    }

    /// 같은 문서가 이미 줄에 있으면 더하지 않는다. 처리 중인 문서면 끝난 뒤 한 번 더 돈다(그 사이 고친 내용 반영)
    pub fn enqueue(&self, document_id: &str, force: bool) {
        let mut queue = self.lock();
        if let Some(existing) = queue.pending.iter_mut().find(|(id, _)| id == document_id) {
            existing.1 |= force;
            return;
        }
        queue.pending.push_back((document_id.to_string(), force));
        if queue.running.as_deref() != Some(document_id) {
            queue.states.insert(document_id.to_string(), JobState::Queued);
        }
        self.inner.wake.notify_one();
    }

    pub fn with_document_lock<T>(&self, _document_id: &str, f: impl FnOnce() -> T) -> T {
        let _guard = self.inner.io_lock.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.inner.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn next_job(&self) -> (String, bool) {
        let mut queue = self.lock();
        loop {
            if let Some(job) = queue.pending.pop_front() {
                queue.running = Some(job.0.clone());
                queue.states.insert(job.0.clone(), JobState::Running { done: 0, total: 0 });
                return job;
            }
            queue = self.inner.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn start(&self, app: tauri::AppHandle) {
        let pipeline = self.clone();
        std::thread::Builder::new()
            .name("scanshelf-pipeline".into())
            .spawn(move || {
                loop {
                    let (document_id, force) = pipeline.next_job();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        pipeline.run_job(&app, &document_id, force)
                    }))
                    .unwrap_or_else(|_| Err("처리 중 예기치 못한 오류".to_string()));
                    {
                        let mut queue = pipeline.lock();
                        queue.running = None;
                        let requeued = queue.pending.iter().any(|(id, _)| id == &document_id);
                        match (&result, requeued) {
                            (_, true) => {
                                queue.states.insert(document_id.clone(), JobState::Queued);
                            }
                            (Ok(()), false) => {
                                queue.states.remove(&document_id);
                            }
                            (Err(message), false) => {
                                queue.states.insert(document_id.clone(), JobState::Failed { message: message.clone() });
                            }
                        }
                    }
                    if result.is_ok() {
                        crate::search::refresh_document(&app, &document_id);
                        // 팜플렛만 처리 직후 AI 정리. 책과 일반 문서는 사람이 누를 때만(docs/ai.md)
                        crate::ai::summarize_if_pamphlet(&app, &document_id);
                    }
                    let _ = app.emit(
                        "document-processed",
                        Finished {
                            document_id,
                            error: result.err(),
                        },
                    );
                }
            })
            .expect("처리 스레드를 시작하지 못했다");
    }

    fn run_job(&self, app: &tauri::AppHandle, document_id: &str, force: bool) -> Result<(), String> {
        let dir = crate::library::document_dir(app, document_id)?;
        let cache = cache_dir(app)?;
        let name = crate::library::load_applied(&dir).map(|(m, _)| m.name).unwrap_or_else(|_| document_id.to_string());
        // 업데이트 설치가 처리 중인 문서를 끊지 않게 등록한다(docs/update.md)
        let _busy = crate::busy::start(format!("처리: {name}"));
        let on_progress = |progress: Progress| {
            if let Some(JobState::Running { done, total }) = self.lock().states.get_mut(document_id) {
                *done = progress.done;
                *total = progress.total;
            }
            let _ = app.emit("processing-progress", progress);
        };
        let models = crate::models::load(app);
        let ctx = ProcessContext {
            dir: &dir,
            cache: &cache,
            io_lock: &self.inner.io_lock,
            force,
            models: &models,
            on_progress: &on_progress,
        };
        process_document(&ctx).map(|_| ())
    }

    /// 앱을 켤 때 다시 처리할 문서를 모두 줄에 세운다(PIPELINE_VERSION이 올랐거나 처리 전 문서)
    pub fn enqueue_stale(&self, app: &tauri::AppHandle) {
        let (Ok(library), Ok(cache)) = (library_dir(app), cache_dir(app)) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&library) else {
            return;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let Ok((manifest, _)) = crate::library::load_applied(&dir) else {
                continue;
            };
            let processing = crate::library::load_processing(&dir);
            if super::stale_pages(&dir, &cache, &manifest, &processing) > 0 {
                self.enqueue(&manifest.id.to_string(), false);
            }
        }
    }
}

#[tauri::command]
pub fn processing_states(app: tauri::AppHandle) -> HashMap<String, JobState> {
    app.state::<Pipeline>().states()
}
