use std::ffi::{CString, c_char, c_long, c_void};
use std::fs;
use std::mem::{MaybeUninit, size_of};
use std::path::Path;
use std::ptr;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::NativePlatformConfig;
use crate::error::{AppResult, AppleMusicDecryptorError as AppError};

use super::cpp::{EndLeaseCallback, PlaybackErrorCallback};
use super::layout::{SharedPtr, StdString, StdStringRef, StdVector, read_std_string};
use super::loader::NativeSymbols;

const FAIRPLAY_CERT: &str = "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";

#[derive(Clone)]
struct CallbackContext {
    symbols: Arc<NativeSymbols>,
    presentation: SharedPtr,
}

static CURRENT_LOGIN: LazyLock<Mutex<Option<Arc<LoginAttempt>>>> =
    LazyLock::new(|| Mutex::new(None));
static CURRENT_CALLBACK_CONTEXT: LazyLock<Mutex<Option<CallbackContext>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ContextKey {
    pub adam: String,
    pub uri: String,
}

#[derive(Clone, Debug)]
pub struct AccountProfile {
    pub storefront_id: String,
    pub dev_token: String,
    pub music_token: String,
    pub offline_available: bool,
}

pub enum LoginWaitState {
    NeedTwoFactor,
    Completed(Box<AppResult<NativeSession>>),
}

struct LoginInner {
    need_two_factor: bool,
    two_factor_code: Option<String>,
    completed: Option<AppResult<NativeSession>>,
    failure_message: Option<String>,
    primary_credentials_submitted: bool,
    repeated_primary_prompt: bool,
}

pub struct LoginAttempt {
    username: String,
    password: String,
    inner: Mutex<LoginInner>,
    condvar: Condvar,
}

impl LoginAttempt {
    pub fn new(username: String, password: String) -> Arc<Self> {
        crate::app_info!(
            "ffi::login",
            "created login attempt: username_len={}, password_len={}",
            username.len(),
            password.len(),
        );
        Arc::new(Self {
            username,
            password,
            inner: Mutex::new(LoginInner {
                need_two_factor: false,
                two_factor_code: None,
                completed: None,
                failure_message: None,
                primary_credentials_submitted: false,
                repeated_primary_prompt: false,
            }),
            condvar: Condvar::new(),
        })
    }

    fn primary_credentials(&self) -> (String, String) {
        (self.username.clone(), self.password.clone())
    }

    fn register_primary_prompt(&self) -> bool {
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        if inner.primary_credentials_submitted {
            if !inner.repeated_primary_prompt {
                crate::app_warn!(
                    "ffi::login",
                    "received a repeated non-2FA credential prompt; treating it as invalid credentials"
                );
                inner.repeated_primary_prompt = true;
            }
            return false;
        }
        inner.primary_credentials_submitted = true;
        true
    }

    fn mark_need_two_factor(&self) {
        crate::app_warn!("ffi::login", "native credential flow requested 2FA");
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        inner.need_two_factor = true;
        self.condvar.notify_all();
    }

    fn fail_initial_login(&self, message: impl Into<String>) {
        let message = message.into();
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        if inner.completed.is_some() || inner.failure_message.is_some() {
            crate::app_warn!(
                "ffi::login",
                "ignoring duplicate login failure signal: {message}"
            );
            return;
        }
        crate::app_error!("ffi::login", "forcing login failure: {message}");
        inner.failure_message = Some(message);
        self.condvar.notify_all();
    }

    pub fn cancel(&self, reason: impl Into<String>) {
        let reason = reason.into();
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        if inner.completed.is_some() || inner.failure_message.is_some() {
            crate::app_warn!(
                "ffi::login",
                "ignoring duplicate login cancellation: {reason}"
            );
            return;
        }
        crate::app_warn!("ffi::login", "canceling login attempt: {reason}");
        inner.failure_message = Some(reason);
        self.condvar.notify_all();
    }

    fn has_failed(&self) -> bool {
        self.inner
            .lock()
            .expect("login mutex poisoned")
            .failure_message
            .is_some()
    }

    fn wait_for_two_factor_code(&self) -> AppResult<String> {
        crate::app_info!("ffi::login", "waiting for 2FA code submission");
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        loop {
            if let Some(message) = inner.failure_message.clone() {
                crate::app_warn!("ffi::login", "2FA wait interrupted: {message}");
                return Err(AppError::Native(message));
            }
            if let Some(code) = inner.two_factor_code.take() {
                crate::app_info!("ffi::login", "received 2FA code: code_len={}", code.len(),);
                return Ok(code);
            }
            inner = self.condvar.wait(inner).expect("login condvar poisoned");
        }
    }

    pub fn submit_two_factor(&self, code: String) -> AppResult<()> {
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        if let Some(message) = inner.failure_message.clone() {
            return Err(AppError::Native(message));
        }
        if !inner.need_two_factor {
            return Err(AppError::UnexpectedTwoFactor);
        }
        crate::app_info!(
            "ffi::login",
            "accepted external 2FA submission: code_len={}",
            code.len(),
        );
        inner.two_factor_code = Some(code);
        self.condvar.notify_all();
        Ok(())
    }

    pub fn finish(&self, result: AppResult<NativeSession>) {
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        if inner.failure_message.is_some() {
            crate::app_warn!(
                "ffi::login",
                "discarding native login result because a failure was already reported"
            );
            return;
        }
        match &result {
            Ok(_) => crate::app_info!("ffi::login", "login attempt finished successfully"),
            Err(error) => crate::app_error!("ffi::login", "login attempt failed: {error}"),
        }
        inner.completed = Some(result);
        self.condvar.notify_all();
    }

    pub fn wait_for_initial_state(&self) -> LoginWaitState {
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        loop {
            if let Some(message) = inner.failure_message.take() {
                crate::app_error!(
                    "ffi::login",
                    "initial login state resolved: forced failure: {message}"
                );
                return LoginWaitState::Completed(Box::new(Err(AppError::Native(message))));
            }
            if let Some(result) = inner.completed.take() {
                crate::app_info!("ffi::login", "initial login state resolved: completed");
                return LoginWaitState::Completed(Box::new(result));
            }
            if inner.need_two_factor {
                crate::app_warn!("ffi::login", "initial login state resolved: need_2fa");
                return LoginWaitState::NeedTwoFactor;
            }
            inner = self.condvar.wait(inner).expect("login condvar poisoned");
        }
    }

    pub fn wait_for_completion(&self) -> AppResult<NativeSession> {
        crate::app_info!("ffi::login", "waiting for login completion after 2FA");
        let mut inner = self.inner.lock().expect("login mutex poisoned");
        loop {
            if let Some(message) = inner.failure_message.take() {
                crate::app_error!(
                    "ffi::login",
                    "2FA login completion forced failure: {message}"
                );
                return Err(AppError::Native(message));
            }
            if let Some(result) = inner.completed.take() {
                match &result {
                    Ok(_) => crate::app_info!("ffi::login", "2FA login completion succeeded"),
                    Err(error) => {
                        crate::app_error!("ffi::login", "2FA login completion failed: {error}")
                    }
                }
                return result;
            }
            inner = self.condvar.wait(inner).expect("login condvar poisoned");
        }
    }
}

pub struct NativePlatform {
    config: NativePlatformConfig,
    symbols: Arc<NativeSymbols>,
    device_guid_obj: *mut c_void,
    _guid: SharedPtr,
}

unsafe impl Send for NativePlatform {}
unsafe impl Sync for NativePlatform {}

pub struct PContextHandle {
    symbols: Arc<NativeSymbols>,
    ctx: SharedPtr,
    kd_context_slot: *mut c_void,
    kd_context: *mut c_void,
}

unsafe impl Send for PContextHandle {}

impl Drop for PContextHandle {
    fn drop(&mut self) {
        unsafe { (self.symbols.shared_ptr_pcontext_drop)(&mut self.ctx) };
    }
}

pub struct NativeSession {
    config: NativePlatformConfig,
    symbols: Arc<NativeSymbols>,
    request_context: SharedPtr,
    presentation: SharedPtr,
    device_guid_obj: *mut c_void,
    lease_manager: Box<[u8; 16]>,
    session_ctrl: *mut c_void,
    session_lock: Mutex<()>,
    _end_lease_callback: EndLeaseCallback,
    _playback_error_callback: PlaybackErrorCallback,
}

unsafe impl Send for NativeSession {}
unsafe impl Sync for NativeSession {}

impl Drop for NativeSession {
    fn drop(&mut self) {
        crate::app_info!("ffi::session", "dropping native session");
        clear_callback_context(self.presentation);
        let automatic = 0_u8;
        unsafe {
            (self.symbols.lease_manager_refresh)(self.lease_manager_ptr(), &automatic);
            (self.symbols.lease_manager_release)(self.lease_manager_ptr());
        }
        crate::app_warn!(
            "ffi::session",
            "skipping SVFootHillSessionCtrl::destroy during drop; native controller teardown is unstable in daemon runtime"
        );
        crate::app_info!(
            "ffi::session",
            "native session core resources released without implicit account logout"
        );
    }
}

impl NativePlatform {
    pub fn bootstrap(config: NativePlatformConfig) -> AppResult<Self> {
        crate::app_info!(
            "ffi::platform",
            "bootstrap start: base_dir={}, library_dir={}, proxy_configured={}",
            config.base_dir.display(),
            config.library_dir.display(),
            config.proxy.is_some(),
        );
        let symbols = Arc::new(NativeSymbols::load(&config.library_dir)?);

        unsafe {
            libc::setenv(c"ANDROID_DNS_MODE".as_ptr(), c"local".as_ptr(), 1);
        }
        crate::app_info!("ffi::platform", "configured ANDROID_DNS_MODE=local");

        if let Some(proxy) = config.proxy.as_deref() {
            let key = CString::new("all_proxy").expect("literal has no NUL");
            let value = CString::new(proxy)
                .map_err(|_| AppError::Message("proxy contains interior NUL".into()))?;
            unsafe { libc::setenv(key.as_ptr(), value.as_ptr(), 1) };
            crate::app_info!("ffi::platform", "configured all_proxy from runtime config");
        }

        let resolvers = [
            c"223.5.5.5".as_ptr() as *const c_char,
            c"223.6.6.6".as_ptr() as *const c_char,
        ];
        unsafe {
            (symbols.resolv_set_nameservers)(0, resolvers.as_ptr(), 2, c".".as_ptr());
        }
        crate::app_info!("ffi::platform", "installed custom DNS resolvers");

        crate::app_info!("ffi::platform", "constructing android_id std::string");
        let android_id = StdStringRef::new(&config.device_info.android_id)?;
        crate::app_info!("ffi::platform", "calling FootHillConfig::config");
        unsafe {
            (symbols.foothill_config)(android_id.as_ptr());
        }
        crate::app_info!(
            "ffi::platform",
            "applied FootHillConfig using device android_id"
        );

        let mut guid = SharedPtr::default();
        crate::app_info!("ffi::platform", "requesting DeviceGUID singleton");
        unsafe { (symbols.device_guid_instance)(&mut guid) };
        let empty = StdStringRef::new("")?;
        let sdk = 29_u32;
        let enabled = 1_u8;
        let mut result = [0_u8; 88];
        crate::app_info!("ffi::platform", "calling DeviceGUID::configure");
        unsafe {
            (symbols.device_guid_configure)(
                &mut result,
                guid.obj,
                android_id.as_ptr(),
                empty.as_ptr(),
                &sdk,
                &enabled,
            );
        }
        crate::app_info!("ffi::platform", "device GUID configured");

        Ok(Self {
            config,
            symbols,
            device_guid_obj: guid.obj,
            _guid: guid,
        })
    }

    pub fn login(&self, attempt: Arc<LoginAttempt>) -> AppResult<NativeSession> {
        crate::app_info!("ffi::platform", "starting native login flow");
        self.clear_login_markers()?;
        let request_context = self.init_request_context()?;
        crate::app_info!("ffi::platform", "request context initialized for login");
        let _login_guard = CurrentLoginGuard::install(Arc::clone(&attempt));
        let (request_context, presentation) = self.authenticate(request_context)?;
        crate::app_info!("ffi::platform", "authenticate flow completed successfully");
        self.build_session(request_context, presentation)
    }

    pub fn restore_session(&self) -> AppResult<Option<NativeSession>> {
        if !persisted_login_markers_exist(&self.config.base_dir) {
            crate::app_info!(
                "ffi::platform",
                "no persisted login markers found; skipping startup session restore"
            );
            return Ok(None);
        }

        crate::app_info!(
            "ffi::platform",
            "persisted login markers detected; attempting startup session restore"
        );
        let request_context = self.init_request_context()?;
        let presentation = self.install_presentation(&request_context);
        Ok(Some(self.build_session(request_context, presentation)?))
    }

    fn clear_login_markers(&self) -> AppResult<()> {
        for marker in ["STOREFRONT_ID", "MUSIC_TOKEN"] {
            let path = self.config.base_dir.join(marker);
            match fs::remove_file(&path) {
                Ok(()) => {
                    crate::app_info!(
                        "ffi::platform",
                        "cleared login cache marker before authenticate: {}",
                        path.display(),
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn install_presentation(&self, request_context: &SharedPtr) -> SharedPtr {
        let mut presentation = SharedPtr::default();
        unsafe {
            (self.symbols.android_presentation_make_shared)(&mut presentation);
            (self.symbols.set_dialog_handler)(presentation.obj, dialog_handler);
            (self.symbols.set_credential_handler)(presentation.obj, credential_handler);
            (self.symbols.request_context_set_presentation)(request_context.obj, &presentation);
        }
        crate::app_info!(
            "ffi::platform",
            "presentation interface installed on request context"
        );
        presentation
    }

    fn build_session(
        &self,
        request_context: SharedPtr,
        presentation: SharedPtr,
    ) -> AppResult<NativeSession> {
        let session = NativeSession::new(
            self.config.clone(),
            Arc::clone(&self.symbols),
            request_context,
            presentation,
            self.device_guid_obj,
        );
        if session.is_err() {
            clear_callback_context(presentation);
        }
        session
    }

    fn init_request_context(&self) -> AppResult<SharedPtr> {
        let path = format!("{}/mpl_db", self.config.base_dir.display());
        crate::app_info!("ffi::platform", "building request context at {}", path);
        let base_dir = StdStringRef::new(&path)?;
        let mut request_context = SharedPtr::default();
        unsafe {
            (self.symbols.request_context_make_shared)(&mut request_context, base_dir.as_ptr());
        }
        crate::app_info!("ffi::platform", "request context shared_ptr created");

        let mut storage = vec![0_u8; 480];
        unsafe {
            *(storage.as_mut_ptr() as *mut *mut c_void) =
                self.symbols.request_context_config_vtable.byte_add(16);
        }
        let config_ptr = unsafe { storage.as_mut_ptr().add(32) as *mut c_void };
        let config_shared = SharedPtr {
            obj: config_ptr,
            ctrl_blk: storage.as_mut_ptr().cast::<c_void>(),
        };
        unsafe { (self.symbols.request_context_config_ctor)(config_ptr) };

        self.set_request_context_strings(config_ptr, &base_dir)?;
        crate::app_info!("ffi::platform", "request context config strings applied");

        unsafe { (self.symbols.request_context_manager_configure)(&request_context) };
        let mut init_buffer = [0_u8; 88];
        unsafe {
            (self.symbols.request_context_init)(
                &mut init_buffer,
                request_context.obj,
                &config_shared,
            )
        };
        crate::app_info!("ffi::platform", "request context manager initialized");

        let fairplay_dir = StdStringRef::new(
            self.config
                .base_dir
                .to_str()
                .ok_or_else(|| AppError::Message("base-dir is not valid UTF-8".into()))?,
        )?;
        unsafe {
            (self.symbols.request_context_set_fairplay_dir)(
                request_context.obj,
                fairplay_dir.as_ptr(),
            );
        }
        crate::app_info!(
            "ffi::platform",
            "fairplay directory attached to request context"
        );

        Ok(request_context)
    }

    fn set_request_context_strings(
        &self,
        config_ptr: *mut c_void,
        base_dir: &StdStringRef,
    ) -> AppResult<()> {
        let info = &self.config.device_info;
        crate::app_info!(
            "ffi::platform",
            "applying device profile: model={}, build={}, locale={}, language={}",
            info.device_model,
            info.build_version,
            info.locale_identifier,
            info.language_identifier,
        );
        unsafe {
            (self.symbols.request_context_set_base_dir)(config_ptr, base_dir.as_ptr());
            (self.symbols.request_context_set_client_id)(
                config_ptr,
                StdStringRef::new(&info.client_identifier)?.as_ptr(),
            );
            (self.symbols.request_context_set_version_id)(
                config_ptr,
                StdStringRef::new(&info.version_identifier)?.as_ptr(),
            );
            (self.symbols.request_context_set_platform_id)(
                config_ptr,
                StdStringRef::new(&info.platform_identifier)?.as_ptr(),
            );
            (self.symbols.request_context_set_product_version)(
                config_ptr,
                StdStringRef::new(&info.product_version)?.as_ptr(),
            );
            (self.symbols.request_context_set_device_model)(
                config_ptr,
                StdStringRef::new(&info.device_model)?.as_ptr(),
            );
            (self.symbols.request_context_set_build_version)(
                config_ptr,
                StdStringRef::new(&info.build_version)?.as_ptr(),
            );
            (self.symbols.request_context_set_locale_id)(
                config_ptr,
                StdStringRef::new(&info.locale_identifier)?.as_ptr(),
            );
            (self.symbols.request_context_set_language_id)(
                config_ptr,
                StdStringRef::new(&info.language_identifier)?.as_ptr(),
            );
        }
        Ok(())
    }
}

impl NativeSession {
    fn new(
        config: NativePlatformConfig,
        symbols: Arc<NativeSymbols>,
        request_context: SharedPtr,
        presentation: SharedPtr,
        device_guid_obj: *mut c_void,
    ) -> AppResult<Self> {
        crate::app_info!("ffi::session", "constructing native session");
        install_callback_context(Arc::clone(&symbols), presentation);
        let end_lease_callback = EndLeaseCallback::new(end_lease_callback);
        let playback_error_callback = PlaybackErrorCallback::new(playback_error_callback);
        let mut lease_manager = Box::new([0_u8; 16]);
        unsafe {
            (symbols.lease_manager_ctor)(
                lease_manager.as_mut_ptr().cast::<c_void>(),
                end_lease_callback.as_ptr(),
                playback_error_callback.as_ptr(),
            );
        }
        let automatic = 1_u8;
        unsafe {
            (symbols.lease_manager_refresh)(
                lease_manager.as_mut_ptr().cast::<c_void>(),
                &automatic,
            );
            (symbols.lease_manager_request_lease)(
                lease_manager.as_mut_ptr().cast::<c_void>(),
                &automatic,
            );
        }
        crate::app_info!(
            "ffi::session",
            "lease manager initialized and lease requested"
        );

        let session_ctrl = unsafe { (symbols.session_ctrl_instance)() };
        if session_ctrl.is_null() {
            return Err(AppError::Native(
                "SVFootHillSessionCtrl::instance returned null".into(),
            ));
        }
        crate::app_info!(
            "ffi::session",
            "session controller acquired: ptr={session_ctrl:p}"
        );

        Ok(Self {
            config,
            symbols,
            request_context,
            presentation,
            device_guid_obj,
            lease_manager,
            session_ctrl,
            session_lock: Mutex::new(()),
            _end_lease_callback: end_lease_callback,
            _playback_error_callback: playback_error_callback,
        })
    }

    pub fn build_context(&self, key: &ContextKey) -> AppResult<PContextHandle> {
        crate::app_info!(
            "ffi::session",
            "building decrypt context: adam={}, uri={}",
            key.adam,
            key.uri,
        );
        let _guard = self.session_lock.lock().expect("session lock poisoned");
        let default_id = StdStringRef::new(&key.adam)?;
        let key_uri = StdStringRef::new(&key.uri)?;
        let key_format = StdStringRef::new("com.apple.streamingkeydelivery")?;
        let key_format_version = StdStringRef::new("1")?;
        let server_uri =
            StdStringRef::new("https://play.itunes.apple.com/WebObjects/MZPlay.woa/music/fps")?;
        let protocol_type = StdStringRef::new("simplified")?;
        let fairplay_cert = StdStringRef::new(FAIRPLAY_CERT)?;

        let mut persistent_key = SharedPtr::default();
        unsafe {
            (self.symbols.session_ctrl_get_persistent_key)(
                &mut persistent_key,
                self.session_ctrl,
                default_id.as_ptr(),
                default_id.as_ptr(),
                key_uri.as_ptr(),
                key_format.as_ptr(),
                key_format_version.as_ptr(),
                server_uri.as_ptr(),
                protocol_type.as_ptr(),
                fairplay_cert.as_ptr(),
            );
        }
        if persistent_key.is_null() {
            return Err(AppError::Native("failed to get persistent key".into()));
        }

        let mut pcontext = SharedPtr::default();
        unsafe {
            (self.symbols.session_ctrl_decrypt_context)(
                &mut pcontext,
                self.session_ctrl,
                persistent_key.obj,
            );
        }
        if pcontext.is_null() {
            return Err(AppError::Native("failed to build decrypt context".into()));
        }

        // The legacy C path does:
        // 1. `*SVFootHillPContext::kdContext()` inside `getKdContext`
        // 2. cast that result back to `void **`
        // 3. dereference again at the decrypt callsite
        //
        // Matching that behavior requires preserving both pointer layers here.
        let kd_context_slot = unsafe { *(self.symbols.pcontext_kd_context)(pcontext.obj) };
        if kd_context_slot.is_null() {
            return Err(AppError::Native(
                "decrypt context returned null kdContext slot".into(),
            ));
        }
        let kd_context = unsafe { *(kd_context_slot.cast::<*mut c_void>()) };
        if kd_context.is_null() {
            return Err(AppError::Native(
                "decrypt context returned null kdContext".into(),
            ));
        }
        crate::app_info!(
            "ffi::session",
            "decrypt context ready: adam={}, kd_context_slot={kd_context_slot:p}, kd_context={kd_context:p}",
            key.adam,
        );

        Ok(PContextHandle {
            symbols: Arc::clone(&self.symbols),
            ctx: pcontext,
            kd_context_slot,
            kd_context,
        })
    }

    pub fn decrypt_sample(
        &self,
        context: &mut PContextHandle,
        mut sample: Vec<u8>,
    ) -> AppResult<Vec<u8>> {
        crate::app_debug!(
            "ffi::session",
            "decrypting sample: kd_context_slot={:p}, kd_context={:p}, bytes={}",
            context.kd_context_slot,
            context.kd_context,
            sample.len(),
        );
        let status = unsafe {
            (self.symbols.decrypt_sample)(
                context.kd_context,
                5,
                sample.as_mut_ptr().cast::<c_void>(),
                sample.as_mut_ptr().cast::<c_void>(),
                sample.len(),
            )
        };
        if status != 0 {
            return Err(AppError::Native(format!(
                "decrypt returned non-zero status {status}"
            )));
        }
        crate::app_debug!(
            "ffi::session",
            "decrypt sample completed: kd_context_slot={:p}, kd_context={:p}, bytes={}",
            context.kd_context_slot,
            context.kd_context,
            sample.len(),
        );
        Ok(sample)
    }

    pub fn load_account_profile(&self) -> AppResult<AccountProfile> {
        crate::app_info!("ffi::session", "loading account profile and native tokens");
        let _guard = self.session_lock.lock().expect("session lock poisoned");

        let storefront_id = self.account_storefront_id_locked()?;
        crate::app_info!(
            "ffi::session",
            "loaded storefront identifier: bytes={}",
            storefront_id.len(),
        );

        let dev_token = self.dev_token_locked()?;
        crate::app_info!(
            "ffi::session",
            "loaded developer token: bytes={}",
            dev_token.len(),
        );

        let guid = self.device_guid_locked()?;
        crate::app_info!("ffi::session", "loaded device guid: bytes={}", guid.len(),);

        let music_token = self.music_user_token_locked(&guid, &dev_token)?;
        crate::app_info!(
            "ffi::session",
            "loaded music user token: bytes={}",
            music_token.len(),
        );

        let offline_available = self.offline_available_locked()?;
        crate::app_info!(
            "ffi::session",
            "detected offline availability: offline_available={offline_available}"
        );

        let profile = AccountProfile {
            storefront_id,
            dev_token,
            music_token,
            offline_available,
        };
        self.write_account_markers_locked(&profile)?;
        crate::app_info!(
            "ffi::session",
            "account profile loaded successfully: storefront_bytes={}, dev_token_bytes={}, music_token_bytes={}, offline_available={}",
            profile.storefront_id.len(),
            profile.dev_token.len(),
            profile.music_token.len(),
            profile.offline_available,
        );
        Ok(profile)
    }

    pub fn resolve_m3u8_url(&self, adam: u64, offline_available: bool) -> AppResult<String> {
        crate::app_info!(
            "ffi::session",
            "resolving m3u8 url: adam={adam}, offline_available={offline_available}"
        );
        let _guard = self.session_lock.lock().expect("session lock poisoned");
        let url = if offline_available {
            self.m3u8_url_download_locked(adam)?
        } else {
            self.m3u8_url_play_locked(adam)?
        };
        crate::app_info!(
            "ffi::session",
            "resolved m3u8 url: adam={adam}, bytes={}",
            url.len(),
        );
        Ok(url)
    }

    #[allow(dead_code)]
    pub fn refresh_lease(&self) -> AppResult<()> {
        crate::app_info!("ffi::session", "refreshing playback lease");
        let _guard = self.session_lock.lock().expect("session lock poisoned");
        let automatic = 1_u8;
        unsafe {
            (self.symbols.lease_manager_refresh)(self.lease_manager_ptr(), &automatic);
            (self.symbols.lease_manager_request_lease)(self.lease_manager_ptr(), &automatic);
        }
        crate::app_warn!(
            "ffi::session",
            "skipping SVFootHillSessionCtrl::resetAllContexts during lease refresh; native controller reset is unstable in daemon runtime"
        );
        crate::app_info!("ffi::session", "playback lease refreshed");
        Ok(())
    }

    pub fn logout(&self) -> AppResult<()> {
        crate::app_info!(
            "ffi::session",
            "logging out native account and clearing cache markers"
        );
        let _guard = self.session_lock.lock().expect("session lock poisoned");

        // `AccountStore::signOutAccount` crashes immediately in the headless daemon
        // runtime during restored-session logout. The daemon only needs to drop its
        // native session state and persistent restore markers so future boots do not
        // recover the old account automatically.
        crate::app_warn!(
            "ffi::session",
            "skipping native account-store sign-out; clearing local native session state only"
        );

        crate::app_warn!(
            "ffi::session",
            "skipping SVFootHillSessionCtrl::resetAllContexts during logout; removing restore markers only"
        );

        for marker in ["STOREFRONT_ID", "MUSIC_TOKEN"] {
            let _ = fs::remove_file(self.config.base_dir.join(marker));
        }
        crate::app_info!("ffi::session", "logout cleanup finished");
        Ok(())
    }

    fn lease_manager_ptr(&self) -> *mut c_void {
        self.lease_manager.as_ptr().cast_mut().cast::<c_void>()
    }

    fn account_storefront_id_locked(&self) -> AppResult<String> {
        crate::app_info!(
            "ffi::session",
            "requesting storefront identifier from request context"
        );
        let mut storefront = MaybeUninit::<StdString>::uninit();
        let url_bag = SharedPtr::default();
        unsafe {
            (self.symbols.request_context_storefront_identifier)(
                storefront.as_mut_ptr(),
                self.request_context.obj,
                &url_bag,
            );
        }
        let storefront = unsafe { storefront.assume_init() };
        let value = read_std_string(&storefront);
        if value.is_empty() {
            return Err(AppError::Native(
                "request context returned an empty storefront identifier".into(),
            ));
        }
        Ok(value)
    }

    fn device_guid_locked(&self) -> AppResult<String> {
        crate::app_info!("ffi::session", "requesting device guid bytes");
        let mut guid_data = [ptr::null_mut(); 2];
        unsafe {
            (self.symbols.device_guid_guid)(&mut guid_data, self.device_guid_obj);
        }
        if guid_data[0].is_null() {
            return Err(AppError::Native(
                "DeviceGUID::guid returned a null data object".into(),
            ));
        }
        let bytes = unsafe { (self.symbols.data_bytes)(guid_data[0]) };
        if bytes.is_null() {
            return Err(AppError::Native(
                "mediaplatform::Data::bytes returned null for device guid".into(),
            ));
        }
        let length = unsafe { (self.symbols.data_length)(guid_data[0]) };
        if length == 0 {
            return Err(AppError::Native("device guid length was zero".into()));
        }
        let guid = String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(bytes.cast::<u8>(), length)
        })
        .into_owned();
        if guid.is_empty() {
            return Err(AppError::Native("device guid bytes were empty".into()));
        }
        Ok(guid)
    }

    fn dev_token_locked(&self) -> AppResult<String> {
        crate::app_info!("ffi::session", "requesting developer token via URLRequest");
        let mut http_storage = Box::new([0_u8; 480]);
        unsafe {
            *(http_storage.as_mut_ptr() as *mut *mut c_void) =
                self.symbols.http_message_vtable.byte_add(16);
            ptr::write_bytes(http_storage.as_mut_ptr().add(8), 0, 16);
        }
        let http_message = SharedPtr {
            obj: unsafe { http_storage.as_mut_ptr().add(32).cast::<c_void>() },
            ctrl_blk: http_storage.as_mut_ptr().cast::<c_void>(),
        };

        let url = StdStringRef::new("https://sf-api-token-service.itunes.apple.com/apiToken")?;
        let method = StdStringRef::new("GET")?;
        unsafe {
            (self.symbols.http_message_ctor)(http_message.obj, url.as_ptr(), method.as_ptr());
        }
        crate::app_info!("ffi::session", "developer token HTTPMessage constructed");

        let mut url_request = Box::new([0_u8; 512]);
        unsafe {
            (self.symbols.url_request_ctor)(
                url_request.as_mut_ptr().cast::<c_void>(),
                &http_message,
                &self.request_context,
            );
        }
        crate::app_info!("ffi::session", "developer token URLRequest constructed");
        let client_id_key = StdStringRef::new("clientId")?;
        let client_id_value = StdStringRef::new("musicAndroid")?;
        let version_key = StdStringRef::new("version")?;
        let version_value = StdStringRef::new("1")?;
        unsafe {
            (self.symbols.url_request_set_parameter)(
                url_request.as_mut_ptr().cast::<c_void>(),
                client_id_key.as_ptr(),
                client_id_value.as_ptr(),
            );
            (self.symbols.url_request_set_parameter)(
                url_request.as_mut_ptr().cast::<c_void>(),
                version_key.as_ptr(),
                version_value.as_ptr(),
            );
            (self.symbols.url_request_run)(url_request.as_mut_ptr().cast::<c_void>());
        }
        crate::app_info!(
            "ffi::session",
            "developer token URLRequest finished running"
        );

        let body = self.url_request_body_locked(url_request.as_mut_ptr().cast::<c_void>())?;
        crate::app_info!(
            "ffi::session",
            "developer token response body captured: bytes={}",
            body.len(),
        );
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let token = value
            .get("token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Native("developer token response was missing token".into()))?
            .to_owned();
        if token.is_empty() {
            return Err(AppError::Native(
                "developer token response was empty".into(),
            ));
        }
        Ok(token)
    }

    fn music_user_token_locked(&self, guid: &str, auth_token: &str) -> AppResult<String> {
        crate::app_info!(
            "ffi::session",
            "requesting music user token via createMusicToken: guid_bytes={}, auth_token_bytes={}",
            guid.len(),
            auth_token.len(),
        );
        let mut http_storage = Box::new([0_u8; 480]);
        unsafe {
            *(http_storage.as_mut_ptr() as *mut *mut c_void) =
                self.symbols.http_message_vtable.byte_add(16);
            ptr::write_bytes(http_storage.as_mut_ptr().add(8), 0, 16);
        }
        let http_message = SharedPtr {
            obj: unsafe { http_storage.as_mut_ptr().add(32).cast::<c_void>() },
            ctrl_blk: http_storage.as_mut_ptr().cast::<c_void>(),
        };

        let url = StdStringRef::new(
            "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/createMusicToken",
        )?;
        let method = StdStringRef::new("POST")?;
        unsafe {
            (self.symbols.http_message_ctor)(http_message.obj, url.as_ptr(), method.as_ptr());
        }
        crate::app_info!("ffi::session", "music token HTTPMessage constructed");

        for (key, value) in [
            ("Content-Type", "application/json; charset=UTF-8"),
            ("Expect", ""),
            ("X-Apple-Requesting-Bundle-Id", "com.apple.android.music"),
            (
                "X-Apple-Requesting-Bundle-Version",
                "Music/4.9 Android/10 model/Samsung S9 build/7663313 (dt:66)",
            ),
        ] {
            let key = StdStringRef::new(key)?;
            let value = StdStringRef::new(value)?;
            unsafe {
                (self.symbols.http_message_set_header)(
                    http_message.obj,
                    key.as_ptr(),
                    value.as_ptr(),
                )
            };
        }
        crate::app_info!("ffi::session", "music token request headers applied");

        let mut body = format!(
            "{{\"guid\":\"{guid}\",\"assertion\":\"{auth_token}\",\"tcc-acceptance-date\":\"{}\"}}",
            current_time_millis()
        )
        .into_bytes();
        unsafe {
            (self.symbols.http_message_set_body_data)(
                http_message.obj,
                body.as_mut_ptr().cast::<c_char>(),
                body.len(),
            );
        }
        crate::app_info!(
            "ffi::session",
            "music token request body installed: bytes={}",
            body.len(),
        );

        let mut url_request = Box::new([0_u8; 512]);
        unsafe {
            (self.symbols.url_request_ctor)(
                url_request.as_mut_ptr().cast::<c_void>(),
                &http_message,
                &self.request_context,
            );
            (self.symbols.url_request_run)(url_request.as_mut_ptr().cast::<c_void>());
        }
        crate::app_info!("ffi::session", "music token URLRequest finished running");

        let body = self.url_request_body_locked(url_request.as_mut_ptr().cast::<c_void>())?;
        crate::app_info!(
            "ffi::session",
            "music token response body captured: bytes={}",
            body.len(),
        );
        let value: serde_json::Value = serde_json::from_str(&body)?;
        let token = value
            .get("music_token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| AppError::Native("music token response was missing music_token".into()))?
            .to_owned();
        if token.is_empty() {
            return Err(AppError::Native("music token response was empty".into()));
        }
        Ok(token)
    }

    fn offline_available_locked(&self) -> AppResult<bool> {
        crate::app_info!("ffi::session", "querying FairPlay subscription status");
        let mut fairplay = SharedPtr::default();
        unsafe {
            (self.symbols.request_context_fairplay)(&mut fairplay, self.request_context.obj);
        }
        if fairplay.is_null() {
            return Err(AppError::Native(
                "RequestContext::fairPlay returned null".into(),
            ));
        }
        let status = unsafe { (self.symbols.fairplay_get_subscription_status)(fairplay.obj) };
        if status.begin.is_null() || status.end.is_null() {
            return Err(AppError::Native(
                "FairPlay subscription status vector was null".into(),
            ));
        }
        let bytes = (status.end as usize).saturating_sub(status.begin as usize);
        if bytes < 32 {
            return Err(AppError::Native(format!(
                "FairPlay subscription status vector was smaller than expected: bytes={bytes}"
            )));
        }
        let state = unsafe { *((status.begin as *const u8).add(24) as *const i32) };
        crate::app_info!(
            "ffi::session",
            "FairPlay subscription status decoded: state={state}"
        );
        Ok(matches!(state, 2 | 3))
    }

    fn m3u8_url_play_locked(&self, adam: u64) -> AppResult<String> {
        crate::app_info!("ffi::session", "requesting playback m3u8 url: adam={adam}");
        let hls = StdStringRef::new("HLS")?;
        let begin = hls.as_ptr().cast_mut().cast::<c_void>();
        let end = unsafe { begin.byte_add(size_of::<StdString>()) };
        let formats = StdVector {
            begin,
            end,
            end_capacity: end,
        };
        let zero = 0_u8;
        let mut response = SharedPtr::default();
        unsafe {
            (self.symbols.playback_lease_manager_request_asset)(
                &mut response,
                self.lease_manager_ptr(),
                &adam,
                &formats,
                &zero,
            );
        }
        if response.is_null() {
            return Err(AppError::Native(format!(
                "requestAsset returned null for adam {adam}"
            )));
        }
        let valid = unsafe { (self.symbols.playback_asset_response_has_valid_asset)(response.obj) };
        if valid == 0 {
            return Err(AppError::Native(format!(
                "requestAsset returned an invalid playback asset for adam {adam}"
            )));
        }

        let playback_asset =
            unsafe { (self.symbols.playback_asset_response_playback_asset)(response.obj) };
        if playback_asset.is_null() || unsafe { (*playback_asset).is_null() } {
            return Err(AppError::Native(format!(
                "playback asset response had no asset for adam {adam}"
            )));
        }
        let mut url = MaybeUninit::<StdString>::uninit();
        unsafe {
            (self.symbols.playback_asset_url_string)(url.as_mut_ptr(), (*playback_asset).obj);
        }
        let url = read_std_string(unsafe { url.assume_init_ref() });
        if url.is_empty() {
            return Err(AppError::Native(format!(
                "playback asset url was empty for adam {adam}"
            )));
        }
        Ok(url)
    }

    fn m3u8_url_download_locked(&self, adam: u64) -> AppResult<String> {
        crate::app_info!("ffi::session", "requesting download m3u8 url: adam={adam}");
        let mut purchase_request = Box::new([0_u8; 1024]);
        unsafe {
            (self.symbols.purchase_request_ctor)(
                purchase_request.as_mut_ptr().cast::<c_void>(),
                &self.request_context,
            );
            (self.symbols.purchase_request_set_process_dialog_actions)(
                purchase_request.as_mut_ptr().cast::<c_void>(),
                1,
            );
        }

        let url_bag_key = StdStringRef::new("subDownload")?;
        let buy_parameters = StdStringRef::new(&format!(
            "salableAdamId={adam}&price=0&pricingParameters=SUBS&productType=S"
        ))?;
        unsafe {
            (self.symbols.purchase_request_set_url_bag_key)(
                purchase_request.as_mut_ptr().cast::<c_void>(),
                url_bag_key.as_ptr(),
            );
            (self.symbols.purchase_request_set_buy_parameters)(
                purchase_request.as_mut_ptr().cast::<c_void>(),
                buy_parameters.as_ptr(),
            );
            (self.symbols.purchase_request_run)(purchase_request.as_mut_ptr().cast::<c_void>());
        }

        let response = unsafe {
            (self.symbols.purchase_request_response)(purchase_request.as_mut_ptr().cast::<c_void>())
        };
        if response.is_null() || unsafe { (*response).is_null() } {
            return Err(AppError::Native(format!(
                "purchase request returned no response for adam {adam}"
            )));
        }
        let error = unsafe { (self.symbols.purchase_response_error)((*response).obj) };
        if !error.is_null() && unsafe { !(*error).is_null() } {
            return Err(AppError::Native(format!(
                "purchase request returned an error response for adam {adam}"
            )));
        }

        let items = unsafe { (self.symbols.purchase_response_items)((*response).obj) };
        if items.begin.is_null() || items.begin == items.end {
            return Err(AppError::Native(format!(
                "purchase response contained no items for adam {adam}"
            )));
        }
        let first_item = items.begin.cast::<SharedPtr>();
        let assets = unsafe { (self.symbols.purchase_item_assets)((*first_item).obj) };
        if assets.begin.is_null() || assets.begin == assets.end {
            return Err(AppError::Native(format!(
                "purchase item contained no assets for adam {adam}"
            )));
        }
        let last_asset = unsafe { (assets.end.cast::<SharedPtr>()).sub(1) };
        let mut url = MaybeUninit::<StdString>::uninit();
        unsafe {
            (self.symbols.purchase_asset_url)(url.as_mut_ptr(), (*last_asset).obj);
        }
        let url = read_std_string(unsafe { url.assume_init_ref() });
        if url.is_empty() {
            return Err(AppError::Native(format!(
                "purchase asset url was empty for adam {adam}"
            )));
        }
        Ok(url)
    }

    fn write_account_markers_locked(&self, profile: &AccountProfile) -> AppResult<()> {
        let storefront_path = self.config.base_dir.join("STOREFRONT_ID");
        let music_token_path = self.config.base_dir.join("MUSIC_TOKEN");
        fs::write(&storefront_path, profile.storefront_id.as_bytes())?;
        fs::write(&music_token_path, profile.music_token.as_bytes())?;
        crate::app_info!(
            "ffi::session",
            "wrote account marker files: storefront_path={}, music_token_path={}, storefront_bytes={}, music_token_bytes={}",
            storefront_path.display(),
            music_token_path.display(),
            profile.storefront_id.len(),
            profile.music_token.len(),
        );
        Ok(())
    }

    fn url_request_body_locked(&self, url_request: *mut c_void) -> AppResult<String> {
        crate::app_info!("ffi::session", "extracting URLRequest response body");
        let error = unsafe { (self.symbols.url_request_error)(url_request) };
        if !error.is_null() && unsafe { !(*error).is_null() } {
            return Err(AppError::Native("URLRequest returned an error".into()));
        }

        let response = unsafe { (self.symbols.url_request_response)(url_request) };
        if response.is_null() || unsafe { (*response).is_null() } {
            return Err(AppError::Native("URLRequest returned no response".into()));
        }
        let underlying =
            unsafe { (self.symbols.url_response_underlying_response)((*response).obj) };
        if underlying.is_null() || unsafe { (*underlying).is_null() } {
            return Err(AppError::Native(
                "URLResponse::underlyingResponse returned null".into(),
            ));
        }

        let http_message_obj = unsafe { (*underlying).obj.cast::<u8>() };
        let data_ptr = unsafe { *(http_message_obj.add(48) as *const *mut c_void) };
        if data_ptr.is_null() {
            return Err(AppError::Native(
                "HTTP response body data pointer was null".into(),
            ));
        }
        let bytes = unsafe { (self.symbols.data_bytes)(data_ptr) };
        if bytes.is_null() {
            return Err(AppError::Native(
                "mediaplatform::Data::bytes returned null for HTTP response body".into(),
            ));
        }
        let length = unsafe { (self.symbols.data_length)(data_ptr) };
        if length == 0 {
            return Err(AppError::Native(
                "HTTP response body length was zero".into(),
            ));
        }
        let text = String::from_utf8_lossy(unsafe {
            std::slice::from_raw_parts(bytes.cast::<u8>(), length)
        })
        .into_owned();
        crate::app_info!(
            "ffi::session",
            "URLRequest response body extraction completed: bytes={}, declared_length={length}",
            text.len(),
        );
        Ok(text)
    }
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_millis() as i64
}

unsafe extern "C" fn dialog_handler(
    dialog_cookie: c_long,
    protocol_dialog: *mut SharedPtr,
    _response_handler: *mut SharedPtr,
) {
    let Some(context) = current_callback_context() else {
        crate::app_error!(
            "ffi::dialog",
            "dialog handler fired without an active callback context"
        );
        return;
    };
    let symbols = Arc::clone(&context.symbols);
    let title = unsafe { read_std_string((symbols.protocol_dialog_title)((*protocol_dialog).obj)) };
    let message =
        unsafe { read_std_string((symbols.protocol_dialog_message)((*protocol_dialog).obj)) };
    crate::app_info!(
        "ffi::dialog",
        "dialogHandler invoked: title={title}, message={message}"
    );

    // Keep the dialog response allocation on the heap for the same reason as
    // credentials responses: the native presentation layer keeps consuming the
    // object after this callback returns.
    let storage = Box::into_raw(Box::new([0_u8; 72]));
    let storage_ptr = unsafe { (*storage).as_mut_ptr() };
    unsafe {
        *(storage_ptr as *mut *mut c_void) = symbols.protocol_dialog_response_vtable.byte_add(16);
        ptr::write_bytes(storage_ptr.add(8), 0, 16);
    }
    let response = SharedPtr {
        obj: unsafe { storage_ptr.add(24).cast::<c_void>() },
        ctrl_blk: storage_ptr.cast::<c_void>(),
    };
    unsafe { (symbols.protocol_dialog_response_ctor)(response.obj) };

    let buttons = unsafe { (symbols.protocol_dialog_buttons)((*protocol_dialog).obj) };
    if title == "Sign In" {
        let mut cursor = unsafe { (*buttons).begin };
        while cursor != unsafe { (*buttons).end } {
            let button = unsafe { &*cursor };
            let button_title =
                unsafe { read_std_string((symbols.protocol_button_title)(button.obj)) };
            if button_title == "Use Existing Apple ID" {
                unsafe {
                    (symbols.protocol_dialog_response_set_selected_button)(response.obj, cursor)
                };
                break;
            }
            cursor = unsafe { cursor.add(1) };
        }
    }

    unsafe {
        (symbols.handle_dialog_response)(context.presentation.obj, &dialog_cookie, &response)
    };
    crate::app_info!("ffi::dialog", "dialog response submitted");
}

unsafe extern "C" fn credential_handler(
    credential_request: *mut SharedPtr,
    _response_handler: *mut SharedPtr,
) {
    let attempt = current_login_attempt();
    let Some(attempt) = attempt else {
        crate::app_error!(
            "ffi::login",
            "credential handler fired without an active login attempt"
        );
        return;
    };

    let Some(context) = current_callback_context() else {
        attempt.fail_initial_login("credential handler fired without an active callback context");
        return;
    };
    let symbols = Arc::clone(&context.symbols);
    let title = unsafe {
        read_std_string((symbols.credentials_request_title)(
            (*credential_request).obj,
        ))
    };
    let message = unsafe {
        read_std_string((symbols.credentials_request_message)(
            (*credential_request).obj,
        ))
    };
    let needs_2fa =
        unsafe { (symbols.credentials_request_needs_2fa)((*credential_request).obj) != 0 };
    crate::app_info!(
        "ffi::login",
        "credentialHandler invoked: title={title}, message={message}, needs_2fa={needs_2fa}"
    );

    if attempt.has_failed() {
        crate::app_warn!(
            "ffi::login",
            "ignoring credential prompt because login attempt is already in a failed state"
        );
        return;
    }

    let (response_type, username, password) = if needs_2fa {
        let (username, password) = attempt.primary_credentials();
        attempt.mark_need_two_factor();
        let final_password = match attempt.wait_for_two_factor_code() {
            Ok(code) => format!("{password}{code}"),
            Err(error) => {
                crate::app_warn!(
                    "ffi::login",
                    "aborting credential response while waiting for 2FA: {error}"
                );
                return;
            }
        };
        (2, Some(username), Some(final_password))
    } else if attempt.register_primary_prompt() {
        let (username, password) = attempt.primary_credentials();
        (2, Some(username), Some(password))
    } else {
        attempt.fail_initial_login(
            "invalid credentials: native login requested another primary credential prompt",
        );
        crate::app_warn!(
            "ffi::login",
            "stopping credential response submission for repeated non-2FA prompt"
        );
        return;
    };

    // Keep the credentials response allocation on the heap to match the original C ABI usage.
    // Native code appears to keep consuming the response after this callback returns.
    let storage = Box::into_raw(Box::new([0_u8; 80]));
    let storage_ptr = unsafe { (*storage).as_mut_ptr() };
    unsafe {
        *(storage_ptr as *mut *mut c_void) = symbols.credentials_response_vtable.byte_add(16);
        ptr::write_bytes(storage_ptr.add(8), 0, 16);
    }
    let response = SharedPtr {
        obj: unsafe { storage_ptr.add(24).cast::<c_void>() },
        ctrl_blk: storage_ptr.cast::<c_void>(),
    };
    unsafe {
        (symbols.credentials_response_ctor)(response.obj);
        if response_type == 2 {
            let username = StdStringRef::new(
                username
                    .as_deref()
                    .expect("username must be available for accept response"),
            )
            .expect("username should not contain NUL");
            let password = StdStringRef::new(
                password
                    .as_deref()
                    .expect("password must be available for accept response"),
            )
            .expect("password should not contain NUL");
            (symbols.credentials_response_set_username)(response.obj, username.as_ptr());
            (symbols.credentials_response_set_password)(response.obj, password.as_ptr());
        }
        (symbols.credentials_response_set_type)(response.obj, response_type);
        (symbols.handle_credentials_response)(context.presentation.obj, &response);
    }
    crate::app_info!(
        "ffi::login",
        "credential response submitted to native layer: response_type={response_type}, ctrl_blk={:p}",
        response.ctrl_blk
    );
}

unsafe extern "C" fn end_lease_callback(code: i32) {
    crate::app_warn!("ffi::session", "end lease callback fired: code={code}");
}

unsafe extern "C" fn playback_error_callback(_value: *mut c_void) {
    crate::app_error!("ffi::session", "playback error callback fired");
}

fn current_login_attempt() -> Option<Arc<LoginAttempt>> {
    CURRENT_LOGIN
        .lock()
        .expect("current login mutex poisoned")
        .clone()
}

fn current_callback_context() -> Option<CallbackContext> {
    CURRENT_CALLBACK_CONTEXT
        .lock()
        .expect("callback context mutex poisoned")
        .clone()
}

fn install_callback_context(symbols: Arc<NativeSymbols>, presentation: SharedPtr) {
    let mut slot = CURRENT_CALLBACK_CONTEXT
        .lock()
        .expect("callback context mutex poisoned");
    *slot = Some(CallbackContext {
        symbols,
        presentation,
    });
}

fn clear_callback_context(presentation: SharedPtr) {
    let mut slot = CURRENT_CALLBACK_CONTEXT
        .lock()
        .expect("callback context mutex poisoned");
    if slot.as_ref().is_some_and(|context| {
        context.presentation.obj == presentation.obj
            && context.presentation.ctrl_blk == presentation.ctrl_blk
    }) {
        slot.take();
    }
}

struct CurrentLoginGuard;

impl CurrentLoginGuard {
    fn install(attempt: Arc<LoginAttempt>) -> Self {
        *CURRENT_LOGIN.lock().expect("current login mutex poisoned") = Some(attempt);
        Self
    }
}

impl Drop for CurrentLoginGuard {
    fn drop(&mut self) {
        CURRENT_LOGIN
            .lock()
            .expect("current login mutex poisoned")
            .take();
    }
}

impl NativePlatform {
    fn authenticate(&self, request_context: SharedPtr) -> AppResult<(SharedPtr, SharedPtr)> {
        crate::app_info!("ffi::platform", "starting authenticate flow");
        let presentation = self.install_presentation(&request_context);
        crate::app_info!(
            "ffi::platform",
            "presentation interface installed for authenticate flow"
        );
        install_callback_context(Arc::clone(&self.symbols), presentation);

        let result = (|| {
            let mut flow = SharedPtr::default();
            unsafe {
                (self.symbols.authenticate_flow_make_shared)(&mut flow, &request_context);
                (self.symbols.authenticate_flow_run)(flow.obj);
            }
            crate::app_info!(
                "ffi::platform",
                "authenticate flow returned control to Rust"
            );

            let response = unsafe { (self.symbols.authenticate_flow_response)(flow.obj) };
            if response.is_null() || unsafe { (*response).is_null() } {
                return Err(AppError::Native(
                    "authenticate flow returned no response".into(),
                ));
            }

            let response_type =
                unsafe { (self.symbols.authenticate_response_type)((*response).obj) };
            crate::app_info!(
                "ffi::platform",
                "authenticate flow response received: response_type={response_type}"
            );
            if response_type == 6 {
                Ok((request_context, presentation))
            } else {
                Err(AppError::Native(format!(
                    "authenticate flow failed with response type {response_type}"
                )))
            }
        })();
        if result.is_err() {
            clear_callback_context(presentation);
        }
        result
    }
}

fn persisted_login_markers_exist(base_dir: &Path) -> bool {
    // We only attempt a startup restore after a previously completed login wrote both markers.
    ["STOREFRONT_ID", "MUSIC_TOKEN"]
        .into_iter()
        .all(|marker| base_dir.join(marker).is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{LoginAttempt, LoginWaitState, persisted_login_markers_exist};

    #[test]
    fn repeated_primary_prompt_is_scoped_to_one_attempt() {
        let first = LoginAttempt::new("user".into(), "pass".into());
        assert!(first.register_primary_prompt());
        assert!(!first.register_primary_prompt());

        let second = LoginAttempt::new("user".into(), "pass".into());
        assert!(second.register_primary_prompt());
    }

    #[test]
    fn forced_failure_resolves_initial_wait_state() {
        let attempt = LoginAttempt::new("user".into(), "pass".into());
        attempt.fail_initial_login("invalid credentials");

        match attempt.wait_for_initial_state() {
            LoginWaitState::Completed(result) => match *result {
                Err(error) => {
                    assert_eq!(error.to_string(), "native error: invalid credentials");
                }
                Ok(_) => panic!("unexpected successful login state"),
            },
            _ => panic!("unexpected login state"),
        }
    }

    #[test]
    fn persisted_login_markers_require_both_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("wrapper-rust-login-markers-{unique}"));
        fs::create_dir_all(&base_dir).expect("temp dir should be created");

        assert!(!persisted_login_markers_exist(&base_dir));

        fs::write(base_dir.join("STOREFRONT_ID"), b"us").expect("storefront marker should exist");
        assert!(!persisted_login_markers_exist(&base_dir));

        fs::write(base_dir.join("MUSIC_TOKEN"), b"token").expect("music marker should exist");
        assert!(persisted_login_markers_exist(&base_dir));

        fs::remove_dir_all(base_dir).expect("temp dir should be removed");
    }
}
