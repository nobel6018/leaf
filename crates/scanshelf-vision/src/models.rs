//! ONNX 모델 목록과 불러오기. 모델 파일은 저장소에 넣지 않고(크기, 라이선스) 앱이 처음 쓸 때 내려받아
//! 모델 폴더에 둔다. 이 크레이트는 네트워크를 쓰지 않는다. 내려받기는 앱이 `MODELS` 목록으로 한다.
//!
//! 실행기는 macOS에서 CoreML을 먼저 붙이고, 붙지 않으면 CPU로 돈다.
//!
//! @see docs/vision.md

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ort::session::Session;
use sha2::{Digest, Sha256};

use crate::VisionError;

/// 내려받을 모델 하나
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    pub id: ModelId,
    /// 모델 폴더 안 파일 이름
    pub file: &'static str,
    /// 내려받을 주소. GitHub 릴리스 자산
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    /// 원본 가중치 출처
    pub source: &'static str,
    /// CoreML 실행기를 붙일지. 붙여서 느려지거나 실패하는 모델은 CPU만 쓴다(측정은 docs/vision.md)
    pub coreml: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelId {
    /// 곡면 보정. 입력 [1,3,712,488] RGB 0~1, 출력 [1,2,45,31] 역방향 격자(-1~1)
    UvDoc,
    /// 인페인팅. 입력 image [1,3,512,512] RGB 0~1, mask [1,1,512,512], 출력 [1,3,512,512] RGB 0~255
    Lama,
}

/// 릴리스 자산 주소. 모델을 바꾸면 태그(models-vN)를 올리고 해시와 크기를 같이 고친다
pub const RELEASE_BASE: &str = "https://github.com/nobel6018/scanshelf/releases/download/models-v1";

macro_rules! release_url {
    ($file:literal) => {
        concat!("https://github.com/nobel6018/scanshelf/releases/download/models-v1/", $file)
    };
}

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: ModelId::UvDoc,
        file: "uvdoc.onnx",
        url: release_url!("uvdoc.onnx"),
        size: 31_602_475,
        sha256: "ba42385a7f881afabc67ca6c4cc854d1506c52b94ee9af4e35261ddf347cea1d",
        license: "MIT",
        source: "https://github.com/tanguymagne/UVDoc (scripts/models/export_uvdoc.py)",
        coreml: true,
    },
    ModelSpec {
        id: ModelId::Lama,
        file: "lama_fp32.onnx",
        url: release_url!("lama_fp32.onnx"),
        size: 208_044_816,
        sha256: "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6",
        license: "Apache-2.0",
        source: "https://huggingface.co/Carve/LaMa-ONNX (big-lama, scripts/models/fetch_lama.py)",
        coreml: false,
    },
];

pub fn spec(id: ModelId) -> &'static ModelSpec {
    MODELS.iter().find(|m| m.id == id).expect("모든 ModelId가 MODELS에 있다")
}

/// 내려받은 파일이 목록과 같은지 크기와 SHA-256으로 확인한다
pub fn verify(path: &Path, spec: &ModelSpec) -> Result<(), VisionError> {
    let meta = std::fs::metadata(path).map_err(|e| VisionError::Model(format!("{}: {e}", path.display())))?;
    if meta.len() != spec.size {
        return Err(VisionError::Model(format!("{} 크기가 다릅니다: {} != {}", spec.file, meta.len(), spec.size)));
    }
    let mut file = File::open(path).map_err(|e| VisionError::Model(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| VisionError::Model(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = hex::encode(hasher.finalize());
    if got != spec.sha256 {
        return Err(VisionError::Model(format!("{} 해시가 다릅니다: {got}", spec.file)));
    }
    Ok(())
}

/// 어떤 실행기로 돌고 있는지. 화면의 진단 정보와 성능 측정에 쓴다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    CoreMl,
    Cpu,
}

pub(crate) struct Loaded {
    pub session: Mutex<Session>,
    pub backend: Backend,
}

/// 모델 폴더와 처음 쓸 때 불러온 세션. 앱 전체에서 하나를 만들어 공유한다(`Sync`)
pub struct Models {
    dir: Option<PathBuf>,
    /// 파일이 없거나 불러오지 못하면 None이 들어가 다음 호출부터 바로 대체 경로로 간다
    uvdoc: OnceLock<Option<Loaded>>,
    lama: OnceLock<Option<Loaded>>,
    allow_coreml: bool,
}

impl Models {
    /// 모델 폴더를 지정한다. 파일 확인과 불러오기는 처음 쓸 때 한다
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: Some(dir.into()), uvdoc: OnceLock::new(), lama: OnceLock::new(), allow_coreml: true }
    }

    /// 모델 없이 고전 알고리즘만 쓴다. 곡면 보정은 건너뛴다
    pub fn none() -> Self {
        Self { dir: None, uvdoc: OnceLock::new(), lama: OnceLock::new(), allow_coreml: false }
    }

    /// CoreML을 끄고 CPU만 쓴다. 비교 측정용
    pub fn cpu_only(mut self) -> Self {
        self.allow_coreml = false;
        self
    }

    /// 모델 폴더에 있어야 할 파일 경로
    pub fn path(&self, id: ModelId) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(spec(id).file))
    }

    /// 파일이 있는지만 본다(해시 확인은 `verify`)
    pub fn available(&self, id: ModelId) -> bool {
        self.path(id).is_some_and(|p| p.is_file())
    }

    pub fn backend(&self, id: ModelId) -> Option<Backend> {
        self.get(id).map(|l| l.backend)
    }

    pub(crate) fn get(&self, id: ModelId) -> Option<&Loaded> {
        let cell = match id {
            ModelId::UvDoc => &self.uvdoc,
            ModelId::Lama => &self.lama,
        };
        cell.get_or_init(|| {
            let path = self.path(id).filter(|p| p.is_file())?;
            load(&path, spec(id).coreml && self.allow_coreml).ok()
        })
        .as_ref()
    }

    /// 불러오기 오류를 그대로 보고 싶을 때(진단용). 성공하면 세션을 캐시하지 않는다
    pub fn try_load(&self, id: ModelId) -> Result<Backend, VisionError> {
        let path = self.path(id).ok_or_else(|| VisionError::Model("모델 폴더가 없습니다".into()))?;
        load(&path, spec(id).coreml && self.allow_coreml).map(|l| l.backend)
    }
}

fn environment() -> Result<&'static ort::environment::Environment, VisionError> {
    static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("scanshelf-vision").build().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| VisionError::Model(e.clone()))
}

/// 실행기를 직접 고른다. ort는 아무것도 안 고르면 macOS에서 CoreML을 자동으로 붙이므로(AutoDevicePolicy 기본값)
/// CPU 경로에는 PreferCPU를 명시한다
fn builder(env: &ort::environment::Environment) -> ort::Result<ort::session::builder::SessionBuilder> {
    use ort::session::builder::AutoDevicePolicy;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    Session::builder(env)?
        .with_intra_threads(threads)
        .map_err(ort::Error::<()>::from)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(ort::Error::<()>::from)
}

fn load(path: &Path, coreml: bool) -> Result<Loaded, VisionError> {
    let env = environment()?;
    let err = |e: ort::Error| VisionError::Model(format!("{}: {e}", path.display()));
    #[cfg(target_os = "macos")]
    if coreml {
        let attempt = (|| -> ort::Result<Session> {
            // 컴파일한 CoreML 모델을 모델 폴더 옆에 남겨 다음 실행부터 세션 준비를 줄인다
            let cache = path.parent().unwrap_or(Path::new(".")).join("coreml-cache");
            let ep = ort::ep::CoreML::default()
                .with_model_format(ort::ep::coreml::ModelFormat::MLProgram)
                .with_model_cache_dir(cache.display())
                .build()
                .error_on_failure();
            builder(env)?
                .with_execution_providers([ep])
                .map_err(ort::Error::<()>::from)?
                .commit_from_file(path)
        })();
        if let Ok(session) = attempt {
            return Ok(Loaded { session: Mutex::new(session), backend: Backend::CoreMl });
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = coreml;
    let session = builder(env).map_err(err)?.commit_from_file(path).map_err(err)?;
    Ok(Loaded { session: Mutex::new(session), backend: Backend::Cpu })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 모든_모델이_목록에_있다() {
        for id in [ModelId::UvDoc, ModelId::Lama] {
            let s = spec(id);
            assert!(s.url.ends_with(s.file));
            assert!(s.url.starts_with(RELEASE_BASE));
            assert_eq!(s.sha256.len(), 64);
        }
    }

    #[test]
    fn 모델_폴더가_없으면_쓰지_않는다() {
        let m = Models::none();
        assert!(!m.available(ModelId::UvDoc));
        assert!(m.get(ModelId::Lama).is_none());
    }

    #[test]
    fn 크기가_다르면_거부한다() {
        let dir = std::env::temp_dir().join(format!("scanshelf-vision-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("uvdoc.onnx");
        std::fs::write(&path, b"not a model").unwrap();
        assert!(verify(&path, spec(ModelId::UvDoc)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
