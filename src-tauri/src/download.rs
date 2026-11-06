//! 모델 이어 받기(조각 병렬, 끊기면 남은 조각만).
//!
//! @see docs/models.md

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use eyre::{bail, eyre, Result, WrapErr};
use serde::Serialize;

pub struct Item {
    pub name: String,
    pub url: String,
    pub dest: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub file: String,
    pub index: usize,
    pub count: usize,
    pub received: u64,
    pub total: u64,
}

/// 한 번에 여는 연결 수. 이 맥(약 50Mbps)에서는 1개와 4~16개의 차이가 10~20%였지만,
/// 연결 하나당 속도를 묶는 서버나 지연이 큰 해외 경로에서는 차이가 커진다. 측정에서 가장 나았던 값이다
const CONNECTIONS: usize = 4;
/// 조각 크기. 끊겼을 때 다시 받는 양의 상한이기도 하다
const CHUNK: u64 = 16 * 1024 * 1024;
const RETRIES: usize = 3;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// 파일을 차례로 받는다. 한 파일은 16MB 조각으로 나눠 연결 4개로 동시에 받는다.
/// 끝난 조각은 옆 파일(.scanshelf-part.json)에 적어 두어, 끊겼다가 다시 부르면 남은 조각만 받는다.
/// 다 받으면 이름을 바꾼다. 이미 받은 파일은 건너뛴다
pub fn download_all(items: &[Item], cancel: &AtomicBool, on_progress: impl Fn(Progress)) -> Result<()> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        // 받는 도중 이 시간 동안 아무것도 안 오면 끊긴 것으로 보고 그 조각을 다시 받는다
        .timeout_read(Duration::from_secs(30))
        .user_agent(concat!("scanshelf/", env!("CARGO_PKG_VERSION")))
        .build();
    let total: u64 = items.iter().map(|i| i.size).sum();
    let mut done: u64 = 0;

    for (index, item) in items.iter().enumerate() {
        let report = |received: u64| Progress {
            file: item.name.clone(),
            index,
            count: items.len(),
            received,
            total,
        };
        if fs::metadata(&item.dest).is_ok_and(|m| m.len() == item.size) {
            done += item.size;
            on_progress(report(done));
            continue;
        }
        download_one(&agent, item, cancel, |got| on_progress(report(done + got)))?;
        done += item.size;
        on_progress(report(done));
    }
    Ok(())
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn download_one(agent: &ureq::Agent, item: &Item, cancel: &AtomicBool, on_progress: impl Fn(u64)) -> Result<()> {
    if let Some(dir) = item.dest.parent() {
        fs::create_dir_all(dir)?;
    }
    // 스크립트(curl -C -)는 .part를 쓴다. 앱의 .part는 미리 전체 크기로 늘려 두고 조각을 채우므로,
    // 같은 이름을 쓰면 curl이 크기만 보고 다 받았다고 판단해 구멍 난 파일을 모델로 설치할 수 있다
    let part = sidecar(&item.dest, ".scanshelf-part");
    let state_path = sidecar(&item.dest, ".scanshelf-part.json");
    let chunks = item.size.div_ceil(CHUNK) as usize;

    // 조각마다 앞에서부터 받은 바이트 수. 기록이 없거나 맞지 않으면 처음부터 받는다
    let mut saved: Vec<u64> = fs::read(&state_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if saved.len() != chunks || fs::metadata(&part).map_or(true, |m| m.len() != item.size) {
        saved = vec![0; chunks];
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .wrap_err_with(|| format!("파일을 쓸 수 없습니다: {}", part.display()))?;
    // 미리 전체 크기로 늘려 두고 조각마다 제자리에 쓴다
    file.set_len(item.size).wrap_err("디스크 공간이 부족합니다")?;

    let progress: Vec<AtomicU64> = saved.iter().map(|&n| AtomicU64::new(n)).collect();
    let pending: Vec<usize> = (0..chunks).filter(|&c| saved[c] < chunk_len(c, item.size)).collect();
    let next = AtomicUsize::new(0);
    let failure: Mutex<Option<eyre::Report>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let workers = CONNECTIONS.min(pending.len());
    let active = AtomicUsize::new(workers);
    let save = || {
        let snapshot: Vec<u64> = progress.iter().map(|p| p.load(Ordering::Relaxed)).collect();
        let _ = fs::write(&state_path, serde_json::to_vec(&snapshot).unwrap_or_default());
    };

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                // 어떻게 끝나든 살아 있는 작업 수를 줄인다
                struct Exit<'a>(&'a AtomicUsize);
                impl Drop for Exit<'_> {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::Relaxed);
                    }
                }
                let _exit = Exit(&active);
                while let Some(&chunk) = pending.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let mut attempt = 0;
                    loop {
                        if stop.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        match fetch_chunk(agent, item, chunk, &file, &progress[chunk], cancel) {
                            Ok(()) => break,
                            Err(_) if cancel.load(Ordering::Relaxed) => return,
                            Err(e) if attempt + 1 >= RETRIES => {
                                stop.store(true, Ordering::Relaxed);
                                failure.lock().unwrap().get_or_insert(e);
                                return;
                            }
                            // 받은 데까지는 progress에 남아 있어, 다시 시도하면 그 다음부터 받는다
                            Err(_) => {
                                attempt += 1;
                                std::thread::sleep(Duration::from_secs(1 << attempt));
                            }
                        }
                    }
                }
            });
        }
        // 받는 동안 이 스레드는 진행률을 알리고, 1초마다 이어 받기 기록을 남긴다
        let mut ticks = 0;
        while active.load(Ordering::Relaxed) > 0 {
            on_progress(progress.iter().map(|p| p.load(Ordering::Relaxed)).sum());
            std::thread::sleep(PROGRESS_INTERVAL);
            ticks += 1;
            if ticks % 10 == 0 {
                save();
            }
        }
    });
    save();

    if cancel.load(Ordering::Relaxed) {
        bail!("내려받기를 취소했습니다. 다시 시작하면 받은 부분부터 이어 받습니다");
    }
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e.wrap_err(format!("{}를 받지 못했습니다. 다시 시작하면 받은 부분부터 이어 받습니다", item.name)));
    }
    if (0..chunks).any(|c| progress[c].load(Ordering::Relaxed) < chunk_len(c, item.size)) {
        bail!("{}의 일부를 받지 못했습니다. 다시 시작해 주세요", item.name);
    }
    drop(file);
    fs::rename(&part, &item.dest)?;
    let _ = fs::remove_file(&state_path);
    Ok(())
}

fn chunk_len(chunk: usize, size: u64) -> u64 {
    let (start, end) = chunk_range(chunk, size);
    end - start + 1
}

/// 조각의 바이트 범위(양 끝 포함)
fn chunk_range(chunk: usize, size: u64) -> (u64, u64) {
    let start = chunk as u64 * CHUNK;
    (start, (start + CHUNK).min(size) - 1)
}

/// 조각 하나를 받은 바이트 다음부터 끝까지 받는다. 받는 대로 done을 늘려 이어 받기 기록에 쓰이게 한다
fn fetch_chunk(agent: &ureq::Agent, item: &Item, chunk: usize, file: &File, done: &AtomicU64, cancel: &AtomicBool) -> Result<()> {
    let (start, end) = chunk_range(chunk, item.size);
    let mut offset = start + done.load(Ordering::Relaxed);
    if offset > end {
        return Ok(());
    }
    let response = agent
        .get(&item.url)
        .set("Range", &format!("bytes={offset}-{end}"))
        .call()
        .map_err(|e| eyre!("요청 실패: {e}"))?;
    if response.status() != 206 {
        bail!("서버가 구간 요청을 지원하지 않습니다(HTTP {})", response.status());
    }
    let mut reader = response.into_reader();
    let mut buf = vec![0u8; 256 * 1024];
    while offset <= end {
        if cancel.load(Ordering::Relaxed) {
            bail!("취소");
        }
        let n = reader.read(&mut buf).wrap_err("연결이 끊겼습니다")?;
        if n == 0 {
            bail!("조각이 예상보다 짧습니다");
        }
        let n = n.min((end + 1 - offset) as usize);
        write_at(file, &buf[..n], offset).wrap_err("디스크에 쓰지 못했습니다. 남은 공간을 확인해 주세요")?;
        offset += n as u64;
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_write(buf, offset)?;
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 마지막_조각은_파일_끝에서_자른다() {
        let size = CHUNK * 2 + 10;

        assert_eq!(chunk_range(0, size), (0, CHUNK - 1));
        assert_eq!(chunk_range(2, size), (CHUNK * 2, CHUNK * 2 + 9));
    }

    #[test]
    fn 기록_파일_이름은_원래_이름_뒤에_붙인다() {
        let p = Path::new("/m/ggml-large-v3.bin");

        assert_eq!(sidecar(p, ".scanshelf-part.json"), Path::new("/m/ggml-large-v3.bin.scanshelf-part.json"));
    }
}
