//! Bonjour(`_leaf._tcp`) 알림. iPhone은 `NWBrowser`로 찾는다.
//!
//! macOS는 시스템 mDNSResponder에 등록한다(`DNSServiceRegister`). 순수 Rust 구현(mdns-sd)은 자체 응답기를 돌리는데,
//! macOS에서는 mDNSResponder가 이미 5353 포트를 쥐고 있어 광고가 루프백에만 보이고 Wi-Fi로 나가지 않았다
//! (v0.1.0에서 iPhone이 Mac을 못 찾은 원인, `dns-sd -B _leaf._tcp`의 if 열이 1뿐이었다).
//! 다른 OS(Windows 등)는 mdns-sd를 쓴다.
//!
//! @see docs/transfer.md

pub const SERVICE_TYPE: &str = "_leaf._tcp.local.";

#[cfg(target_os = "macos")]
pub use macos::Advertiser;
#[cfg(not(target_os = "macos"))]
pub use portable::Advertiser;

/// TXT 기록. iPhone은 `api`가 1인 Mac만 고른다
fn txt_properties() -> [(&'static str, &'static str); 2] {
    [("api", "1"), ("version", env!("CARGO_PKG_VERSION"))]
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CString, c_char, c_void};
    use std::ptr;

    type DnsServiceRef = *mut c_void;

    // dns_sd.h. libSystem에 들어 있어 따로 링크하지 않는다
    unsafe extern "C" {
        fn DNSServiceRegister(
            sd_ref: *mut DnsServiceRef,
            flags: u32,
            interface_index: u32,
            name: *const c_char,
            regtype: *const c_char,
            domain: *const c_char,
            host: *const c_char,
            port_network_order: u16,
            txt_len: u16,
            txt_record: *const c_void,
            callback: *const c_void,
            context: *mut c_void,
        ) -> i32;
        fn DNSServiceRefDeallocate(sd_ref: DnsServiceRef);
    }

    /// 등록 핸들을 들고 있는 동안 광고된다. 버리면(drop) 시스템이 광고를 거둔다
    pub struct Advertiser {
        sd_ref: DnsServiceRef,
    }

    // 핸들은 등록과 해제에만 쓰고 다른 스레드에서 동시에 만지지 않는다
    unsafe impl Send for Advertiser {}
    unsafe impl Sync for Advertiser {}

    impl Advertiser {
        pub fn start(mac_name: &str, port: u16) -> Result<Advertiser, String> {
            let name =
                CString::new(mac_name).map_err(|_| "Mac 이름에 NUL 문자가 있다".to_string())?;
            let regtype = CString::new("_leaf._tcp").expect("고정 문자열");
            let txt = super::txt_record();
            let mut sd_ref: DnsServiceRef = ptr::null_mut();
            // 이름, 호스트, 도메인을 시스템 기본값(이 Mac의 이름과 .local)으로 두고, 콜백 없이 등록한다.
            // 인터페이스 0은 모든 인터페이스(Wi-Fi, 유선)에 알린다는 뜻이다
            let error = unsafe {
                DNSServiceRegister(
                    &mut sd_ref,
                    0,
                    0,
                    name.as_ptr(),
                    regtype.as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    port.to_be(),
                    txt.len() as u16,
                    txt.as_ptr().cast(),
                    ptr::null(),
                    ptr::null_mut(),
                )
            };
            if error != 0 || sd_ref.is_null() {
                return Err(format!("Bonjour 등록 실패(DNSServiceRegister {error})"));
            }
            Ok(Advertiser { sd_ref })
        }
    }

    impl Drop for Advertiser {
        fn drop(&mut self) {
            unsafe { DNSServiceRefDeallocate(self.sd_ref) };
        }
    }
}

/// DNS TXT 기록 형식: 항목마다 길이 1바이트 + `key=value`
fn txt_record() -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in txt_properties() {
        let entry = format!("{key}={value}");
        out.push(entry.len() as u8);
        out.extend_from_slice(entry.as_bytes());
    }
    out
}

#[cfg(not(target_os = "macos"))]
mod portable {
    use super::{SERVICE_TYPE, host_label, txt_properties};
    use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

    pub struct Advertiser {
        daemon: ServiceDaemon,
        fullname: String,
    }

    impl Advertiser {
        pub fn start(mac_name: &str, port: u16) -> Result<Advertiser, String> {
            let daemon =
                ServiceDaemon::new().map_err(|e| format!("mDNS를 시작하지 못했다: {e}"))?;
            daemon
                .disable_interface(IfKind::IPv6)
                .map_err(|e| format!("mDNS 설정 실패: {e}"))?;
            let host = format!("{}.local.", host_label(mac_name));
            let properties = txt_properties();
            let info = ServiceInfo::new(SERVICE_TYPE, mac_name, &host, "", port, &properties[..])
                .map_err(|e| format!("Bonjour 정보를 만들지 못했다: {e}"))?
                .enable_addr_auto();
            let fullname = info.get_fullname().to_string();
            daemon
                .register(info)
                .map_err(|e| format!("Bonjour 등록 실패: {e}"))?;
            Ok(Advertiser { daemon, fullname })
        }
    }

    impl Drop for Advertiser {
        fn drop(&mut self) {
            let _ = self.daemon.unregister(&self.fullname);
            let _ = self.daemon.shutdown();
        }
    }
}

/// DNS 호스트 이름 한 칸(영문, 숫자, 하이픈). Mac 이름이 한글뿐이면 기본값을 쓴다.
/// 실제 Mac 호스트 이름과 겹치지 않게 `leaf-`를 붙인다
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn host_label(mac_name: &str) -> String {
    let cleaned: String = mac_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    let mut label = String::from("leaf");
    if !trimmed.is_empty() {
        label.push('-');
        label.push_str(trimmed);
    }
    label.chars().take(63).collect()
}

#[cfg(test)]
mod tests {
    use super::{host_label, txt_record};

    #[test]
    fn txt_기록은_길이를_앞에_붙인_항목들이다() {
        let txt = txt_record();
        assert_eq!(&txt[..6], b"\x05api=1");
        assert_eq!(txt[6] as usize, txt.len() - 7);
    }

    #[test]
    fn 호스트_이름은_영문_숫자_하이픈만_쓴다() {
        assert_eq!(
            host_label("Leedo's MacBook Pro"),
            "leaf-leedo-s-macbook-pro"
        );
        assert_eq!(host_label("이도의 맥북"), "leaf");
    }

    /// 실제 네트워크에 알려지는지 손으로 확인할 때만 돌린다.
    /// `cargo test -p leaf bonjour_광고 -- --ignored` 중에 `dns-sd -B _leaf._tcp`의 if 열에 Wi-Fi(en0) 번호가 보여야 한다
    #[test]
    #[ignore]
    fn bonjour_광고를_잠깐_띄운다() {
        let _advertiser = super::Advertiser::start("leaf 확인용", 45999).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(8));
    }
}
