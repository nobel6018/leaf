//! 합성 3단 접지로 세 면 나누기를 잰다. 실제 팜플렛은 쓰지 않는다(저작권, 회사 자료).
//! 면 셋을 글자로 그리고, 접은 자국(가는 어두운 선), 면마다 다른 밝기(빛 받는 각도), 남은 원근을 넣는다.
//!
//! ```sh
//! cargo test -p scanshelf-vision --release --test trifold -- --nocapture   # 정확도 표
//! ```
//!
//! @see docs/vision.md

mod common;

use common::*;
use image::{Rgb, RgbImage};
use scanshelf_vision::fold::{MIN_FOLD_CONFIDENCE, Trifold, find_folds, split_trifold};
use scanshelf_vision::perspective::{Corners, Homography, Interpolation, PixelPoint, warp_to_size};

const H: u32 = 1060;

#[derive(Debug, Clone, Copy)]
struct Scene {
    seed: u64,
    /// 접은 자국 어둡기(0~1)
    crease: f32,
    /// 가운데 면과 오른쪽 면이 덜 밝은 정도(0~1)
    tone: [f32; 2],
    /// 남은 원근(꼭짓점 흔들기, 폭 대비)
    jitter: f32,
    /// 면 하나에 사진 상자를 넣는다
    photo: bool,
}

/// 합성 사진과 정답 접힌 선(위쪽 x, 아래쪽 x) 두 개
fn render(scene: Scene) -> (RgbImage, [(f32, f32); 2]) {
    let mut rng = Lcg::new(scene.seed);
    // 안으로 접히는 면은 조금 좁다. 어느 면이 좁은지는 사진마다 다르다
    let mut widths = [500u32, 500, 500];
    widths[rng.range(0, 3) as usize] -= rng.range(6, 22);
    let w: u32 = widths.iter().sum();
    let folds = [widths[0] as f32, (widths[0] + widths[1]) as f32];
    let mut flat = RgbImage::new(w, H);
    let mut x0 = 0;
    for (k, pw) in widths.iter().enumerate() {
        let mut panel = render_page(*pw, H, scene.seed * 10 + k as u64);
        if scene.photo && k == 1 {
            // 면 위쪽 절반을 덮는 사진(어두운 무늬). 면 가장자리까지 닿는다
            for y in H / 8..H / 2 {
                for x in 0..*pw {
                    let v = 90 + ((x / 7 + y / 11) % 5) as u8 * 18;
                    panel.put_pixel(x, y, Rgb([v, v.saturating_sub(20), v / 2 + 40]));
                }
            }
        }
        image::imageops::replace(&mut flat, &panel, x0 as i64, 0);
        x0 += pw;
    }
    // 면마다 밝기, 접은 자국, 전체 조명 기울기
    let tones = [1.0, 1.0 - scene.tone[0], 1.0 - scene.tone[1]];
    for (x, y, p) in flat.enumerate_pixels_mut() {
        let xf = x as f32;
        let panel = folds.iter().filter(|f| xf >= **f).count();
        let d = folds.iter().map(|f| (xf - f).abs()).fold(f32::MAX, f32::min);
        let crease = 1.0 - scene.crease * (-d * d / (2.0 * 1.5f32.powi(2))).exp();
        let light = 1.0 - 0.06 * (y as f32 / H as f32) - 0.04 * (xf / w as f32);
        let s = tones[panel] * crease * light;
        p.0 = p.0.map(|v| (v as f32 * s).clamp(0.0, 255.0) as u8);
    }
    if scene.jitter == 0.0 {
        return (flat, folds.map(|f| (f, f)));
    }
    // 꼭짓점을 조금 잘못 잡은 원근 보정: 흔든 네 꼭짓점 안쪽을 같은 크기 직사각형으로 편다
    let j = |rng: &mut Lcg, size: u32| (rng.range(0, 1000) as f32 / 1000.0 * 1.5 - 0.5) * scene.jitter * size as f32;
    let (wf, hf) = (w as f32, H as f32);
    let src = [
        (j(&mut rng, w), j(&mut rng, H)),
        (wf - j(&mut rng, w), j(&mut rng, H)),
        (wf - j(&mut rng, w), hf - j(&mut rng, H)),
        (j(&mut rng, w), hf - j(&mut rng, H)),
    ];
    let p = |(x, y): (f32, f32)| PixelPoint::new(x as f64, y as f64);
    let corners = Corners { top_left: p(src[0]), top_right: p(src[1]), bottom_right: p(src[2]), bottom_left: p(src[3]) };
    let out = warp_to_size(&flat, &corners, w, H, Interpolation::Bicubic).unwrap();
    // 정답: 원본의 세로 접힌 선을 출력으로 옮긴 직선의 위아래 끝
    let rect = [PixelPoint::new(0.0, 0.0), PixelPoint::new(wf as f64, 0.0), PixelPoint::new(wf as f64, hf as f64), PixelPoint::new(0.0, hf as f64)];
    let to_out = Homography::from_points(corners.points(), rect).unwrap();
    let truth = folds.map(|f| {
        let a = to_out.apply(PixelPoint::new(f as f64, 0.0));
        let b = to_out.apply(PixelPoint::new(f as f64, hf as f64));
        let x_at = |y: f64| a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y);
        (x_at(0.0) as f32, x_at(hf as f64) as f32)
    });
    (out, truth)
}

struct Outcome {
    split: bool,
    /// 두 선의 위아래 끝 중 가장 큰 오차(화소)
    error: f32,
}

fn evaluate(scene: Scene) -> Outcome {
    let (img, truth) = render(scene);
    let folds = find_folds(&img);
    let error = [(folds.left, truth[0]), (folds.right, truth[1])]
        .iter()
        .map(|(f, t)| (f.x_top - t.0).abs().max((f.x_bottom - t.1).abs()))
        .fold(0.0, f32::max);
    Outcome { split: folds.confidence >= MIN_FOLD_CONFIDENCE, error }
}

/// 조건별: (이름, 사진들)
fn conditions() -> Vec<(&'static str, Vec<Scene>)> {
    let base = |seed: u64| Scene { seed, crease: 0.2, tone: [0.0, 0.0], jitter: 0.0, photo: false };
    // 최적화 없는 빌드에서는 조건마다 3장만(표에 실은 수치는 --release 8장)
    let per = if cfg!(debug_assertions) { 3 } else { 8 };
    let set = |f: &dyn Fn(u64) -> Scene| (1..=per).map(f).collect::<Vec<_>>();
    vec![
        ("자국 뚜렷", set(&|s| base(s))),
        ("자국 옅음(6%)", set(&|s| Scene { crease: 0.06, ..base(s + 100) })),
        ("자국 아주 옅음(3%)", set(&|s| Scene { crease: 0.03, ..base(s + 600) })),
        ("밝기 계단만(4%)", set(&|s| Scene { crease: 0.0, tone: [0.04, 0.0], ..base(s + 200) })),
        ("단서 없음(여백만)", set(&|s| Scene { crease: 0.0, ..base(s + 300) })),
        ("원근 남음(2%) + 옅은 자국", set(&|s| Scene { crease: 0.1, jitter: 0.02, ..base(s + 400) })),
        ("원근 남음(4%) + 옅은 자국", set(&|s| Scene { crease: 0.1, jitter: 0.04, ..base(s + 700) })),
        ("면에 사진 + 옅은 자국", set(&|s| Scene { crease: 0.1, photo: true, tone: [0.03, 0.03], ..base(s + 500) })),
    ]
}

/// 오차가 이보다 작으면 면이 제대로 나뉜 것으로 본다(폭 1500의 0.8%, A4 접지에서 약 2.4mm)
const OK_ERROR: f32 = 12.0;

#[test]
fn 합성_3단_접지_정확도() {
    let mut total_ok = 0;
    let mut total = 0;
    for (name, scenes) in conditions() {
        let outcomes: Vec<Outcome> = scenes.iter().map(|s| evaluate(*s)).collect();
        let ok = outcomes.iter().filter(|o| o.split && o.error < OK_ERROR).count();
        let split = outcomes.iter().filter(|o| o.split).count();
        let mean = outcomes.iter().map(|o| o.error).sum::<f32>() / outcomes.len() as f32;
        let max = outcomes.iter().map(|o| o.error).fold(0.0, f32::max);
        eprintln!("{name}: 정확히 나눔 {ok}/{} | 나눔 {split} | 오차 평균 {mean:.1}px 최대 {max:.1}px", outcomes.len());
        total_ok += ok;
        total += outcomes.len();
        // 잘못 나누는 것(나눴는데 오차가 큼)은 어느 조건에서도 없어야 한다. 못 찾으면 통째로 남아 손해가 작다
        for (s, o) in scenes.iter().zip(&outcomes) {
            assert!(!o.split || o.error < OK_ERROR * 2.0, "{name} seed {}: 잘못 나눔, 오차 {:.1}", s.seed, o.error);
        }
    }
    eprintln!("전체: {total_ok}/{total}");
    assert!(total_ok * 100 >= total * 85, "{total_ok}/{total}");
}

#[test]
fn 접지가_아닌_사진은_나누지_않는다() {
    let mut negatives: Vec<(&str, RgbImage)> = Vec::new();
    for seed in 1..=4u64 {
        // 세로로 긴 한 페이지
        negatives.push(("한 페이지", render_page(760, H, seed)));
        // 가로로 긴 한 장(글줄이 끝까지 이어진다)
        negatives.push(("가로 한 장", render_page(1500, H, seed + 10)));
    }
    // 펼친 책(가운데 접힌 선 하나)
    for seed in 1..=4u64 {
        let (l, r) = (render_page(740, H, seed + 20), render_page(740, H, seed + 21));
        let mut book = RgbImage::new(1480, H);
        image::imageops::replace(&mut book, &l, 0, 0);
        image::imageops::replace(&mut book, &r, 740, 0);
        for (x, _, p) in book.enumerate_pixels_mut() {
            let d = x as f32 - 740.0;
            let s = 1.0 - 0.35 * (-d * d / (2.0 * 25.0f32.powi(2))).exp();
            p.0 = p.0.map(|v| (v as f32 * s) as u8);
        }
        negatives.push(("펼친 책", book));
    }
    let mut wrong = Vec::new();
    for (name, img) in &negatives {
        if let Trifold::Panels { folds, .. } = split_trifold(img) {
            wrong.push(format!("{name}: {folds:?}"));
        }
    }
    eprintln!("접지 아님 {}장 중 잘못 나눔 {}", negatives.len(), wrong.len());
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn 세_면을_자르면_폭이_정답과_맞다() {
    let (img, truth) = render(Scene { seed: 7, crease: 0.15, tone: [0.02, 0.0], jitter: 0.0, photo: false });
    let Trifold::Panels { panels, .. } = split_trifold(&img) else { panic!("나눠야 한다") };
    let want = [truth[0].0, truth[1].0 - truth[0].0, img.width() as f32 - truth[1].0];
    for (p, w) in panels.iter().zip(want) {
        assert!((p.width() as f32 - w).abs() < 6.0, "{} vs {w}", p.width());
        assert_eq!(p.height(), H);
    }
}

#[test]
#[ignore = "속도 측정. --release로 부른다"]
fn 세_면_나누기_속도() {
    let (img, _) = render(Scene { seed: 5, crease: 0.1, tone: [0.0, 0.0], jitter: 0.0, photo: false });
    // 출력 상한(긴 변 4000)에 맞춘 크기
    let big = image::imageops::resize(&img, 4000, 2830, image::imageops::FilterType::Triangle);
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let split = split_trifold(&big);
        times.push(start.elapsed().as_millis());
        assert!(matches!(split, Trifold::Panels { .. }));
    }
    times.sort();
    eprintln!("4000x2830 세 면 나누기(찾기 + 자르기) 중앙값 {} ms", times[2]);
}

#[test]
#[ignore = "전후 이미지를 SCANSHELF_EVAL_OUT에 남긴다"]
fn 합성_3단_접지_이미지_남기기() {
    let Some(dir) = std::env::var_os("SCANSHELF_EVAL_OUT") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (img, _) = render(Scene { seed: 3, crease: 0.1, tone: [0.03, 0.03], jitter: 0.02, photo: true });
    img.save(dir.join("trifold_input.png")).unwrap();
    if let Trifold::Panels { panels, .. } = split_trifold(&img) {
        for (k, p) in panels.iter().enumerate() {
            p.save(dir.join(format!("trifold_panel_{}.png", k + 1))).unwrap();
        }
    }
}
