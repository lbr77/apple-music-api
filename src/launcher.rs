use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};

const ROOTFS_DIR: &str = "./rootfs";
const INNER_LINKER: &str = "/system/bin/linker64";
const DEV_NULL: &str = "/dev/null";
const DEV_URANDOM: &str = "/dev/urandom";
const CAP_SYS_ADMIN_BIT: u64 = 1 << 21;

static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

extern "C" fn forward_signal(signum: libc::c_int) {
    let child_pid = CHILD_PID.load(Ordering::Relaxed);
    if child_pid > 0 {
        unsafe {
            libc::kill(child_pid, signum);
        }
    }
}

pub fn run_launcher() -> AppResult<i32> {
    install_signal_handlers()?;

    let config = AppConfig::parse()?;
    let can_unshare_pid = has_cap_sys_admin()?;
    crate::app_info!(
        "launcher",
        "preparing chroot launcher: rootfs={}, base_dir={}, daemon_addr={}",
        ROOTFS_DIR,
        config.base_dir.display(),
        config.daemon_addr(),
    );

    std::env::set_current_dir(ROOTFS_DIR)?;
    chroot_current_dir()?;
    std::env::set_current_dir("/")?;

    ensure_dev_null()?;
    ensure_char_device(DEV_URANDOM, libc::makedev(0x1, 0x9), false)?;
    ensure_executable(INNER_LINKER)?;

    if can_unshare_pid {
        unshare_pid_namespace()?;
    }

    let child_pid = fork_process()?;
    if child_pid == 0 {
        run_child(&config);
    }

    CHILD_PID.store(child_pid, Ordering::Relaxed);
    wait_for_child(child_pid)
}

fn install_signal_handlers() -> AppResult<()> {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let handler = forward_signal as *const () as usize;
        let result = unsafe { libc::signal(signal, handler) };
        if result == libc::SIG_ERR {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

fn chroot_current_dir() -> AppResult<()> {
    let current_dir = CString::new(".").expect("static string does not contain NUL");
    let result = unsafe { libc::chroot(current_dir.as_ptr()) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

fn ensure_executable(path: &str) -> AppResult<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn ensure_dev_null() -> AppResult<()> {
    match ensure_char_device(DEV_NULL, libc::makedev(0x1, 0x3), true) {
        Ok(()) => Ok(()),
        Err(error) => {
            crate::app_warn!(
                "launcher",
                "falling back to regular file for {}: {}",
                DEV_NULL,
                error
            );
            OpenOptions::new()
                .create(true)
                .read(true)
                .truncate(false)
                .open(DEV_NULL)?;
            Ok(())
        }
    }
}

fn ensure_char_device(path: &str, device: libc::dev_t, allow_fallback: bool) -> AppResult<()> {
    let c_path = CString::new(path).expect("static path does not contain NUL");
    let result = unsafe { libc::mknod(c_path.as_ptr(), libc::S_IFCHR | 0o666, device) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return Ok(());
    }
    if allow_fallback {
        return Err(error.into());
    }
    if Path::new(path).exists() {
        return Ok(());
    }
    Err(error.into())
}

fn has_cap_sys_admin() -> AppResult<bool> {
    let status = fs::read_to_string("/proc/self/status")?;
    let cap_eff = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .map(str::trim)
        .ok_or_else(|| AppError::Message("CapEff missing from /proc/self/status".into()))?;
    let cap_eff = u64::from_str_radix(cap_eff, 16)
        .map_err(|error| AppError::Message(format!("invalid CapEff value: {error}")))?;
    Ok((cap_eff & CAP_SYS_ADMIN_BIT) != 0)
}

fn unshare_pid_namespace() -> AppResult<()> {
    crate::app_info!("launcher", "enabling CLONE_NEWPID before fork");
    let result = unsafe { libc::unshare(libc::CLONE_NEWPID) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

fn fork_process() -> AppResult<i32> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(pid)
    }
}

fn run_child(config: &AppConfig) -> ! {
    let result = (|| -> AppResult<()> {
        fs::create_dir_all(config.base_dir.join("mpl_db"))?;
        crate::run_server_process_blocking(config.clone())
    })();

    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            crate::app_error!("launcher", "failed to start child server: {error}");
            std::process::exit(1);
        }
    }
}

fn wait_for_child(child_pid: i32) -> AppResult<i32> {
    let mut status = 0;
    loop {
        let result = unsafe { libc::waitpid(child_pid, &mut status, 0) };
        if result == child_pid {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(error.into());
    }

    if libc::WIFEXITED(status) {
        return Ok(libc::WEXITSTATUS(status));
    }
    if libc::WIFSIGNALED(status) {
        return Ok(128 + libc::WTERMSIG(status));
    }
    Ok(1)
}
