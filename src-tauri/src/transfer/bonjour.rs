//! Bonjour(`_leaf._tcp`) 알림. iPhone은 `NWBrowser`로 찾는다.
//!
//! macOS의 mDNSResponder 대신 순수 Rust 구현(mdns-sd)을 쓴다. Windows에도 그대로 붙이려는 선택이다.
//! IPv4만 알린다. 서버가 `0.0.0.0`에 열려 있어 IPv6 주소로는 붙을 수 없다.
//!
//! @see docs/transfer.md

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

pub const SERVICE_TYPE: &str = "_leaf._tcp.local.";

pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Advertiser {
    pub fn start(mac_name: &str, port: u16) -> Result<Advertiser, String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS를 시작하지 못했다: {e}"))?;
        daemon
            .disable_interface(IfKind::IPv6)
            .map_err(|e| format!("mDNS 설정 실패: {e}"))?;
        let host = format!("{}.local.", host_label(mac_name));
        let properties = [("api", "1"), ("version", env!("CARGO_PKG_VERSION"))];
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

/// DNS 호스트 이름 한 칸(영문, 숫자, 하이픈). Mac 이름이 한글뿐이면 기본값을 쓴다.
/// 실제 Mac 호스트 이름과 겹치지 않게 `leaf-`를 붙인다
fn host_label(mac_name: &str) -> String {
    let cleaned: String = mac_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
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
    use super::host_label;

    #[test]
    fn 호스트_이름은_영문_숫자_하이픈만_쓴다() {
        assert_eq!(host_label("Leedo's MacBook Pro"), "leaf-leedo-s-macbook-pro");
        assert_eq!(host_label("이도의 맥북"), "leaf");
    }
}
