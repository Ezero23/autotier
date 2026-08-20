use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use autotier_lib::{update_settings, AppSettings, AppState, Database, MultiAppConfig};

type HomeSlot = Mutex<Option<&'static Path>>;

fn home_slot() -> &'static HomeSlot {
    static HOME: OnceLock<HomeSlot> = OnceLock::new();
    HOME.get_or_init(|| Mutex::new(None))
}

fn install_home(base: PathBuf) -> &'static Path {
    if base.exists() {
        let _ = std::fs::remove_dir_all(&base);
    }
    std::fs::create_dir_all(&base).expect("create test home");
    // Windows 上 `dirs::home_dir()` 不受 HOME/USERPROFILE 影响（走 Known Folder API），
    // 用 CC_SWITCH_TEST_HOME 显式覆盖，以确保测试不会污染真实用户目录。
    std::env::set_var("CC_SWITCH_TEST_HOME", &base);
    std::env::set_var("HOME", &base);
    #[cfg(windows)]
    std::env::set_var("USERPROFILE", &base);
    Box::leak(base.into_boxed_path())
}

fn new_process_test_home() -> PathBuf {
    static RESET_COUNT: AtomicUsize = AtomicUsize::new(0);
    let sequence = RESET_COUNT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "cc-switch-test-home-{}-{}",
        std::process::id(),
        sequence
    ))
}

/// 为测试设置隔离的 HOME 目录，避免污染真实用户数据。
pub fn ensure_test_home() -> &'static Path {
    let mut slot = home_slot().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(home) = *slot {
        return home;
    }
    let base = std::env::var_os("CC_SWITCH_TEST_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(new_process_test_home);
    let home = install_home(base);
    *slot = Some(home);
    home
}

/// 清理测试目录中生成的配置文件与缓存。
pub fn reset_test_fs() {
    // 普通 integration-test binary 之间无法共享进程内 Mutex；每次 reset 换一个目录，
    // 这样旧的 SQLite/Writer 句柄即使尚未退出，也不会阻塞下一组测试。
    // WSL2 契约必须保持 workflow 注入的 UNC home/temp，因此不轮换该目录。
    let home = if std::env::var_os("CC_SWITCH_WSL_TEST_DIR").is_some() {
        ensure_test_home()
    } else {
        let home = install_home(new_process_test_home());
        let mut slot = home_slot().lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(home);
        home
    };
    for sub in [
        ".claude",
        ".codex",
        ".cc-switch",
        ".autotier",
        ".gemini",
        ".grok",
        ".config",
        ".openclaw",
        "profiles",
    ] {
        let path = home.join(sub);
        if path.exists() {
            force_writable_tree(&path);
            if let Err(err) = std::fs::remove_dir_all(&path) {
                eprintln!("failed to clean {}: {}", path.display(), err);
            }
        }
    }
    let claude_json = home.join(".claude.json");
    if claude_json.exists() {
        force_writable_tree(&claude_json);
        let _ = std::fs::remove_file(&claude_json);
    }

    // 重置内存中的设置缓存，确保测试环境不受上一次调用影响
    let _ = update_settings(AppSettings::default());
}

/// Best-effort chmod so readonly artifacts (e.g. shadow_db_failure) do not block cleanup.
fn force_writable_tree(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = (|| -> Result<(), std::io::Error> {
            if path.is_dir() {
                for entry in std::fs::read_dir(path)? {
                    force_writable_tree(&entry?.path());
                }
            }
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms)
        })();
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[allow(dead_code)]
pub fn enable_codex_official_auth_preservation() {
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");
}

/// 进程内互斥锁，避免同一个测试 binary 内并发写入 HOME 目录。
/// 不依赖它跨 binary 同步；普通测试 HOME 已按进程隔离，WSL2 契约只运行一个 lib test binary。
pub fn test_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

/// 创建测试用的 AppState，包含一个空的数据库
#[allow(dead_code)]
pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    ensure_test_crypto();
    let db = Arc::new(Database::init()?);
    Ok(AppState::new(db))
}

/// 创建测试用的 AppState，并从 MultiAppConfig 迁移数据
#[allow(dead_code)]
pub fn create_test_state_with_config(
    config: &MultiAppConfig,
) -> Result<AppState, Box<dyn std::error::Error>> {
    ensure_test_crypto();
    let db = Arc::new(Database::init()?);
    db.migrate_from_json(config)?;
    Ok(AppState::new(db))
}

/// Integration tests bypass Tauri setup; install rustls ring provider once.
fn ensure_test_crypto() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
