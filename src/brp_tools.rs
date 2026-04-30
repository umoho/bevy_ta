use std::{
    env,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use bevy_brp_extras::{BrpExtrasPlugin, DEFAULT_REMOTE_PORT};

const BRP_PORT_ENV: &str = "BRP_EXTRAS_PORT";
const SCREENSHOT_DIR_ENV: &str = "BEVY_TA_CAPTURE_DIR";
const DEFAULT_CAPTURE_DIR: &str = "assets/private/captures";

pub struct BrpToolsPlugin;

impl Plugin for BrpToolsPlugin {
    fn build(&self, app: &mut App) {
        let port = effective_brp_port();
        app.add_plugins(BrpExtrasPlugin::with_port(port))
            .init_resource::<CaptureCounter>()
            .add_systems(Startup, log_brp_usage)
            .add_systems(Update, capture_screenshot_on_hotkey);
    }
}

#[derive(Resource, Default)]
struct CaptureCounter(u32);

fn log_brp_usage() {
    let port = effective_brp_port();
    let capture_dir = capture_directory();
    info!("BRP 截图已启用，端口 http://127.0.0.1:{port}");
    info!(
        "截图接口: curl -s http://127.0.0.1:{port} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"brp_extras/screenshot\",\"params\":{{\"path\":\"{}/capture.png\"}},\"id\":1}}'",
        capture_dir.display()
    );
    info!("也可以按 F12 直接导出当前窗口截图");
}

fn capture_screenshot_on_hotkey(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut counter: ResMut<CaptureCounter>,
) {
    if !keyboard.just_pressed(KeyCode::F12) {
        return;
    }

    let path = next_capture_path(counter.0);
    counter.0 += 1;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    info!("已请求导出截图 {}", path.display());
}

fn effective_brp_port() -> u16 {
    env::var(BRP_PORT_ENV)
        .ok()
        .and_then(|text| text.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(DEFAULT_REMOTE_PORT)
}

fn capture_directory() -> PathBuf {
    env::var(SCREENSHOT_DIR_ENV)
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CAPTURE_DIR))
}

fn next_capture_path(counter: u32) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let file_name = format!("capture-{timestamp}-{counter}.png");
    capture_directory().join(Path::new(&file_name))
}
