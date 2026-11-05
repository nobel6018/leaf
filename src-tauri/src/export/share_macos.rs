//! macOS 공유 시트(NSSharingServicePicker). AirDrop, 메일, 메시지로 내보낸 파일을 바로 보낸다.
//!
//! @see docs/export.md

use std::path::Path;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSSharingServicePicker, NSView};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

/// NSMinYEdge: 단추 아래쪽에 띄운다(뒤집히지 않은 좌표계 기준)
const BELOW: NSRectEdge = NSRectEdge(1);

/// `x`, `y`는 창 왼쪽 위 기준 화면 좌표(CSS 픽셀 = 포인트). 반드시 메인 스레드에서 부른다
pub fn show_share_picker(ns_view: *mut std::ffi::c_void, file: &Path, x: f64, y: f64) {
    if ns_view.is_null() {
        return;
    }
    let view: &NSView = unsafe { &*(ns_view as *const NSView) };
    let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
    let item: Retained<AnyObject> = url.into();
    let items = NSArray::from_retained_slice(&[item]);
    let picker = unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items) };
    let bounds = view.bounds();
    // 내용 보기는 원점이 왼쪽 아래다
    let flipped_y = if view.isFlipped() { y } else { bounds.size.height - y };
    let rect = NSRect::new(NSPoint::new(x, flipped_y), NSSize::new(1.0, 1.0));
    picker.showRelativeToRect_ofView_preferredEdge(rect, view, BELOW);
    // 공유 창이 떠 있는 동안 picker가 살아 있어야 한다. 한 번에 수십 바이트라 놓아 둔다
    std::mem::forget(picker);
}
