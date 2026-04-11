use std::sync::{Arc, Mutex};

use crate::ffi::LoginAttempt;
use crate::runtime::SessionRuntime;

#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Arc<SessionRuntime>>>,
    pending_login: Mutex<Option<Arc<LoginAttempt>>>,
}

impl AppState {
    pub fn replace_session(&self, session: Arc<SessionRuntime>) -> Option<Arc<SessionRuntime>> {
        self.clear_pending_login();
        let previous = self
            .session
            .lock()
            .expect("session mutex poisoned")
            .replace(session);
        crate::app_info!(
            "runtime::state",
            "installed active session: replaced_existing={}",
            previous.is_some(),
        );
        previous
    }

    pub fn clear_session(&self) -> Option<Arc<SessionRuntime>> {
        let previous = self.session.lock().expect("session mutex poisoned").take();
        crate::app_info!(
            "runtime::state",
            "cleared active session: had_session={}",
            previous.is_some(),
        );
        previous
    }

    pub fn session(&self) -> Option<Arc<SessionRuntime>> {
        self.session.lock().expect("session mutex poisoned").clone()
    }

    pub fn set_pending_login(&self, attempt: Arc<LoginAttempt>) {
        crate::app_warn!("runtime::state", "stored pending login waiting for 2FA");
        self.pending_login
            .lock()
            .expect("pending login mutex poisoned")
            .replace(attempt);
    }

    pub fn pending_login(&self) -> Option<Arc<LoginAttempt>> {
        self.pending_login
            .lock()
            .expect("pending login mutex poisoned")
            .clone()
    }

    pub fn take_pending_login(&self) -> Option<Arc<LoginAttempt>> {
        let previous = self
            .pending_login
            .lock()
            .expect("pending login mutex poisoned")
            .take();
        crate::app_info!(
            "runtime::state",
            "took pending login: had_pending_login={}",
            previous.is_some(),
        );
        previous
    }

    pub fn clear_pending_login(&self) -> Option<Arc<LoginAttempt>> {
        let previous = self
            .pending_login
            .lock()
            .expect("pending login mutex poisoned")
            .take();
        if previous.is_some() {
            crate::app_warn!("runtime::state", "cleared pending login slot");
        }
        previous
    }
}
