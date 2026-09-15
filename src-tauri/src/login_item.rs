//! The login item: whether macOS launches Talkie when the user signs in.
//!
//! Registration goes through `SMAppService` (macOS 13+), which lists the app
//! by name under System Settings › General › Login Items and tracks it by
//! bundle rather than by path. The system, not the settings store, is the
//! truth for this switch: the user can flip it off in System Settings without
//! telling Talkie, so `enabled` asks macOS every time.

use anyhow::{anyhow, Result};

/// Ask macOS whether Talkie is registered to open at login.
#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    // SAFETY: `mainAppService` and `status` take no arguments and have no
    // preconditions beyond running inside a bundle; outside one, status
    // reports `NotFound`.
    let status = unsafe { SMAppService::mainAppService().status() };
    status == SMAppServiceStatus::Enabled
}

/// Register or unregister the login item so it matches `enabled`.
///
/// Errors carry the system's own message: a `cargo tauri dev` binary is not a
/// bundle and cannot be a login item, and macOS says so.
#[cfg(target_os = "macos")]
pub fn apply(enabled: bool) -> Result<()> {
    use objc2_service_management::{SMAppService, SMAppServiceStatus};

    // SAFETY: plain method calls on the singleton; the `Result` wraps the
    // `NSError` out-parameter the way the framework defines it.
    unsafe {
        let service = SMAppService::mainAppService();
        let status = service.status();
        // `NotFound` is a bundle macOS has never been asked about — every
        // fresh install, and every `cargo tauri dev` binary. Switching such a
        // thing off is already done, and `unregister` would only complain.
        match (enabled, status) {
            (true, SMAppServiceStatus::Enabled)
            | (false, SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound) => Ok(()),
            (true, _) => {
                service
                    .registerAndReturnError()
                    .map_err(|e| anyhow!("{}", e.localizedDescription()))?;
                // A user who once switched Talkie off in System Settings has
                // to switch it back on there; registering again cannot
                // override that choice, so take them to the switch.
                if service.status() == SMAppServiceStatus::RequiresApproval {
                    SMAppService::openSystemSettingsLoginItems();
                }
                Ok(())
            }
            (false, _) => service
                .unregisterAndReturnError()
                .map_err(|e| anyhow!("{}", e.localizedDescription())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn enabled() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn apply(enabled: bool) -> Result<()> {
    if enabled {
        Err(anyhow!("launch at login is only implemented on macOS"))
    } else {
        Ok(())
    }
}
