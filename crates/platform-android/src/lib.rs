use remote_env_core::collector::CapabilityState;
#[cfg(target_os = "android")]
use remote_env_core::collector::CollectorEvent;

pub fn bridge_capability() -> CapabilityState {
    if cfg!(target_os = "android") {
        CapabilityState::Available
    } else {
        CapabilityState::Unavailable
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tauri::{Manager, Runtime, plugin::PluginHandle};

    #[derive(Debug, Deserialize)]
    struct NativeEvent {
        data_type: String,
        timestamp_ms: i64,
        data: serde_json::Value,
    }

    #[derive(Debug, Deserialize)]
    struct NativeBatch {
        events: Vec<NativeEvent>,
    }

    pub struct AndroidCollector<R: Runtime> {
        handle: PluginHandle<R>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PersistenceSettings {
        pub master_enabled: bool,
        pub foreground_enabled: bool,
        pub auto_start_enabled: bool,
        pub hide_from_recents: bool,
        pub accessibility_enabled: bool,
        pub battery_optimization_ignored: bool,
        pub root_enabled: bool,
        pub root_available: bool,
        pub device_admin_active: bool,
        pub device_owner_active: bool,
        pub profile_owner_active: bool,
        pub dhizuku_compat_enabled: bool,
        pub dhizuku_supported: bool,
        pub android_api_level: u32,
        pub background_location_granted: bool,
        pub location_enabled: bool,
        pub device_owner_command: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DhizukuApp {
        pub package_name: String,
        pub label: String,
        pub uid: i32,
        pub allowed: bool,
    }

    #[derive(Debug, Deserialize)]
    struct DhizukuAppsResponse {
        apps: Vec<DhizukuApp>,
    }

    impl<R: Runtime> Clone for AndroidCollector<R> {
        fn clone(&self) -> Self {
            Self {
                handle: self.handle.clone(),
            }
        }
    }

    impl<R: Runtime> AndroidCollector<R> {
        pub fn set_master_enabled(&self, enabled: bool) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>(
                    "setMasterEnabled",
                    serde_json::json!({"enabled": enabled}),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn collect_all(&self) -> Result<Vec<CollectorEvent>, String> {
            let batch: NativeBatch = self
                .handle
                .run_mobile_plugin("collectAll", ())
                .map_err(|error| error.to_string())?;
            Ok(batch
                .events
                .into_iter()
                .map(|value| CollectorEvent {
                    data_type: value.data_type,
                    timestamp_ms: value.timestamp_ms,
                    data: value.data,
                })
                .collect())
        }

        pub fn persistence_settings(&self) -> Result<PersistenceSettings, String> {
            self.handle
                .run_mobile_plugin("getPersistenceSettings", ())
                .map_err(|error| error.to_string())
        }

        pub fn set_foreground_enabled(&self, enabled: bool) -> Result<(), String> {
            let _: serde_json::Value = self
                .handle
                .run_mobile_plugin(
                    "setForegroundEnabled",
                    serde_json::json!({"enabled": enabled}),
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        }

        pub fn set_auto_start_enabled(&self, enabled: bool) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>(
                    "setAutoStartEnabled",
                    serde_json::json!({"enabled": enabled}),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn request_auto_start_permission(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestAutoStartPermission", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn set_hide_from_recents(&self, enabled: bool) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>(
                    "setHideFromRecents",
                    serde_json::json!({"enabled": enabled}),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn request_accessibility_permission(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestAccessibilityPermission", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn request_home_settings(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestHomeSettings", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn request_battery_optimization_exemption(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestBatteryOptimizationExemption", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn set_root_enabled(&self, enabled: bool) -> Result<(), String> {
            let _: serde_json::Value = self
                .handle
                .run_mobile_plugin("setRootEnabled", serde_json::json!({"enabled": enabled}))
                .map_err(|error| error.to_string())?;
            Ok(())
        }

        pub fn request_device_admin(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestDeviceAdmin", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn request_background_location(&self) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>("requestBackgroundLocation", ())
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn set_dhizuku_compat_enabled(&self, enabled: bool) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>(
                    "setDhizukuCompatEnabled",
                    serde_json::json!({"enabled": enabled}),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }

        pub fn list_dhizuku_apps(&self) -> Result<Vec<DhizukuApp>, String> {
            self.handle
                .run_mobile_plugin::<DhizukuAppsResponse>("listDhizukuApps", ())
                .map(|response| response.apps)
                .map_err(|error| error.to_string())
        }

        pub fn set_dhizuku_app_authorization(
            &self,
            package_name: &str,
            allowed: bool,
        ) -> Result<(), String> {
            self.handle
                .run_mobile_plugin::<serde_json::Value>(
                    "setDhizukuAppAuthorization",
                    serde_json::json!({"packageName": package_name, "allowed": allowed}),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }

    pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("native-collector")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.remoteenv.collector.nativecollector",
                    "EnvironmentCollectorPlugin",
                )?;
                app.manage(AndroidCollector { handle });
                Ok(())
            })
            .build()
    }
}

#[cfg(target_os = "android")]
pub use android::{AndroidCollector, DhizukuApp, PersistenceSettings, init};
