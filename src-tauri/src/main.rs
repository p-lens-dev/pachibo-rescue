//! The desktop app: finds the iPhone backups on this computer, and writes
//! ぱち簿's records out of the one chosen as a `.pcb` file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod android;

use std::path::{Path, PathBuf};

use ios_backup::manifest::Backup;
use ios_backup::summary::{Summary, summarize};
use ios_backup::{BackupError, discovery};
use pachibo::RescueError;
use serde::Serialize;

/// A backup as the screen shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupView {
    path: String,
    device_name: Option<String>,
    product_type: Option<String>,
    ios_version: Option<String>,
    made_at: Option<i64>,
    encrypted: bool,
    has_pachibo: bool,
}

/// What went wrong, for the screen to put in words.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum Problem {
    PermissionDenied { path: String },
    Missing { path: String },
    Unreadable { path: String, detail: String },
    PasswordNeeded,
    WrongPassword,
    NotInBackup,
    NotPachiboData,
    Broken { detail: String },
    WriteFailed { path: String, detail: String },
    ScrapeFailed { detail: String, log: Option<String> },
    PhoneLost { detail: String, log: Option<String> },
    AdbMissing { detail: String },
    Busy,
    /// The person stopped it before anything was read.
    Cancelled,
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

impl From<BackupError> for Problem {
    fn from(error: BackupError) -> Self {
        match error {
            BackupError::PermissionDenied { path } => Self::PermissionDenied { path: shown(&path) },
            BackupError::Missing { path } => Self::Missing { path: shown(&path) },
            BackupError::Io { path, error } => Self::Unreadable { path: shown(&path), detail: error.to_string() },
            BackupError::Plist { path, detail } => Self::Unreadable { path: shown(&path), detail },
            BackupError::PasswordNeeded => Self::PasswordNeeded,
            BackupError::WrongPassword => Self::WrongPassword,
            BackupError::Keybag { detail } | BackupError::Manifest { detail } => Self::Broken { detail },
        }
    }
}

impl From<RescueError> for Problem {
    fn from(error: RescueError) -> Self {
        match error {
            RescueError::NotInBackup => Self::NotInBackup,
            RescueError::NotPachiboData { .. } => Self::NotPachiboData,
            RescueError::Backup(error) => error.into(),
        }
    }
}

fn view_of(summary: Summary) -> BackupView {
    BackupView {
        has_pachibo: summary.apps.iter().any(|app| app == pachibo::BUNDLE_ID),
        path: shown(&summary.path),
        device_name: summary.device_name,
        product_type: summary.product_type,
        ios_version: summary.ios_version,
        made_at: summary.made_at,
        encrypted: summary.encrypted,
    }
}

fn views_in(roots: &[PathBuf]) -> Result<Vec<BackupView>, Problem> {
    let mut views = Vec::new();
    for root in roots {
        for backup in discovery::backups_in(root)? {
            if let Ok(summary) = summarize(&backup) {
                views.push(view_of(summary));
            }
        }
    }
    views.sort_by_key(|view| std::cmp::Reverse(view.made_at));
    Ok(views)
}

/// The backups in the folders Finder, iTunes and Apple Devices keep them in, newest first.
#[tauri::command]
fn find_backups() -> Result<Vec<BackupView>, Problem> {
    views_in(&discovery::default_roots())
}

/// Whether this computer has a usual place for backups: none on Linux.
#[tauri::command]
fn has_usual_place() -> bool {
    !discovery::default_roots().is_empty()
}

/// The backups in a folder the person chose, newest first.
#[tauri::command]
fn backups_in_folder(path: String) -> Result<Vec<BackupView>, Problem> {
    views_in(&[PathBuf::from(path)])
}

fn rescue_to(backup: &Path, password: Option<&str>, out: &Path) -> Result<u64, Problem> {
    let rescued = pachibo::rescue(&Backup::open(backup, password)?)?;
    std::fs::write(out, &rescued.pcb).map_err(|error| Problem::WriteFailed { path: shown(out), detail: error.to_string() })?;
    Ok(rescued.tallies)
}

/// ぱち簿's records out of the backup, written to `out`; how many tallies they hold.
/// Off the main thread: unlocking an encrypted backup takes seconds.
#[tauri::command]
async fn rescue(path: String, password: Option<String>, out: String) -> Result<u64, Problem> {
    tauri::async_runtime::spawn_blocking(move || rescue_to(Path::new(&path), password.as_deref().filter(|password| !password.is_empty()), Path::new(&out)))
        .await
        .map_err(|error| Problem::Broken { detail: error.to_string() })?
}

fn main() {
    let ran = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            android::log_panics(app.handle());
            Ok(())
        })
        .manage(android::Cancel::default())
        .manage(android::Driving::default())
        .manage(android::BundledAdb::default())
        .invoke_handler(tauri::generate_handler![has_usual_place, find_backups, backups_in_folder, rescue, android::android_phones, android::android_survey, android::android_rescue, android::android_cancel])
        .build(tauri::generate_context!());
    match ran {
        Ok(app) => app.run(|app, event| match event {
            // Closing while the phone is being read waits for the reading to
            // stop, so the phone is left as it was found.
            tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::CloseRequested { api, .. }, .. } => {
                if android::close_after_stopping(app) {
                    api.prevent_close();
                }
            }
            tauri::RunEvent::ExitRequested { api, .. } => {
                if android::close_after_stopping(app) {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => android::stop_bundled_adb(app),
            _ => {}
        }),
        Err(error) => {
            eprintln!("pachibo-rescue could not start: {error}");
            std::process::exit(1);
        }
    }
}
