use std::sync::atomic::{AtomicI32, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(
    target_os = "windows",
    all(target_os = "macos", feature = "unix-file-copy-paste")
))]
use hbb_common::ResultType;
#[cfg(any(target_os = "windows", feature = "unix-file-copy-paste"))]
use hbb_common::{allow_err, log};
use hbb_common::{
    lazy_static,
    tokio::sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        Mutex as TokioMutex,
    },
};
use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(
    target_os = "windows",
    all(target_os = "macos", feature = "unix-file-copy-paste")
))]
pub mod context_send;
pub mod platform;
#[cfg(any(
    target_os = "windows",
    all(target_os = "macos", feature = "unix-file-copy-paste")
))]
pub use context_send::*;

#[cfg(target_os = "windows")]
const ERR_CODE_SERVER_FUNCTION_NONE: u32 = 0x00000001;
#[cfg(target_os = "windows")]
const ERR_CODE_INVALID_PARAMETER: u32 = 0x00000002;
#[cfg(target_os = "windows")]
const ERR_CODE_SEND_MSG: u32 = 0x00000003;

#[cfg(any(
    target_os = "windows",
    all(target_os = "macos", feature = "unix-file-copy-paste")
))]
pub(crate) use platform::create_cliprdr_context;

pub struct ProgressPercent {
    pub percent: f64,
    pub is_canceled: bool,
    pub is_failed: bool,
}

// to-do: This trait may be removed, because unix file copy paste does not need it.
/// Ability to handle Clipboard File from remote rustdesk client
///
/// # Note
/// There actually should be 2 parts to implement a useable clipboard file service,
/// but this only contains the RPC server part.
/// The local listener and transport part is too platform specific to wrap up in typeclasses.
pub trait CliprdrServiceContext: Send + Sync {
    /// set to be stopped
    fn set_is_stopped(&mut self) -> Result<(), CliprdrError>;
    /// Returns true if the context has been stopped and can no longer serve
    /// clipboard requests. A stopped context must be reset or recreated.
    fn is_stopped(&self) -> bool;
    /// Clear the stopped flag so the context can be reused. This is useful
    /// when the context is otherwise healthy but was temporarily stopped by
    /// a permission change or error path.
    fn reset(&mut self) -> Result<(), CliprdrError>;
    /// clear the content on clipboard
    fn empty_clipboard(&mut self, conn_id: i32) -> Result<bool, CliprdrError>;
    /// run as a server for clipboard RPC
    fn server_clip_file(&mut self, conn_id: i32, msg: ClipboardFile) -> Result<(), CliprdrError>;
    /// get the progress of the paste task.
    fn get_progress_percent(&self) -> Option<ProgressPercent>;
    /// cancel the paste task.
    fn cancel(&mut self);
}

#[derive(Error, Debug)]
pub enum CliprdrError {
    #[error("invalid cliprdr name")]
    CliprdrName,
    #[error("failed to init cliprdr")]
    CliprdrInit,
    #[error("cliprdr out of memory")]
    CliprdrOutOfMemory,
    #[error("cliprdr internal error")]
    ClipboardInternalError,
    #[error("cliprdr occupied")]
    ClipboardOccupied,
    #[error("conversion failure")]
    ConversionFailure,
    #[error("failure to read clipboard")]
    OpenClipboard,
    #[error("failure to read file metadata or content, path: {path}, err: {err}")]
    FileError { path: String, err: std::io::Error },
    #[error("invalid request: {description}")]
    InvalidRequest { description: String },
    #[error("common request: {description}")]
    CommonError { description: String },
    #[error("unknown cliprdr error")]
    Unknown(u32),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", content = "c")]
pub enum ClipboardFile {
    NotifyCallback {
        r#type: String,
        title: String,
        text: String,
    },
    MonitorReady,
    FormatList {
        format_list: Vec<(i32, String)>,
    },
    FormatListResponse {
        msg_flags: i32,
    },
    FormatDataRequest {
        requested_format_id: i32,
    },
    FormatDataResponse {
        msg_flags: i32,
        format_data: Vec<u8>,
    },
    FileContentsRequest {
        stream_id: i32,
        list_index: i32,
        dw_flags: i32,
        n_position_low: i32,
        n_position_high: i32,
        cb_requested: i32,
        have_clip_data_id: bool,
        clip_data_id: i32,
    },
    FileContentsResponse {
        msg_flags: i32,
        stream_id: i32,
        requested_data: Vec<u8>,
    },
    TryEmpty,
    Files {
        files: Vec<(String, u64)>,
    },
}

struct MsgChannel {
    peer_id: String,
    conn_id: i32,
    #[allow(dead_code)]
    sender: UnboundedSender<ClipboardFile>,
    receiver: Arc<TokioMutex<UnboundedReceiver<ClipboardFile>>>,
}

lazy_static::lazy_static! {
    static ref VEC_MSG_CHANNEL: RwLock<Vec<MsgChannel>> = Default::default();
    static ref CLIENT_CONN_ID_COUNTER: Mutex<i32> = Mutex::new(0);
}

// Cross-session clipboard relay state (control side).
// `CLIPBOARD_OWNER` records which session provided the current file clipboard
// content: 0 means the local machine, otherwise the conn_id of the remote
// session that injected the latest FormatList.
static CLIPBOARD_OWNER: AtomicI32 = AtomicI32::new(0);
// `PENDING_REQUESTER` records the conn_id of a session that is pasting files
// whose content lives on another session (a relayed request is in flight).
static PENDING_REQUESTER: AtomicI32 = AtomicI32::new(0);
// `INJECTING_REMOTE_CLIPBOARD_OWNER` is temporarily set to the conn_id of a
// remote session while that session's FormatList is being injected into the
// local Windows clipboard. The local clipboard monitor callback is asynchronous
// and would otherwise reset CLIPBOARD_OWNER to 0, breaking cross-session
// paste relay (A->B). The callback uses this value to restore the owner.
static INJECTING_REMOTE_CLIPBOARD_OWNER: AtomicI32 = AtomicI32::new(0);

#[inline]
pub fn get_clipboard_owner() -> i32 {
    CLIPBOARD_OWNER.load(AtomicOrdering::SeqCst)
}

#[inline]
pub fn set_clipboard_owner(conn_id: i32) {
    CLIPBOARD_OWNER.store(conn_id, AtomicOrdering::SeqCst);
}

#[inline]
pub fn get_pending_requester() -> i32 {
    PENDING_REQUESTER.load(AtomicOrdering::SeqCst)
}

#[inline]
pub fn set_pending_requester(conn_id: i32) {
    PENDING_REQUESTER.store(conn_id, AtomicOrdering::SeqCst);
}

#[inline]
pub fn set_injecting_remote_clipboard_owner(conn_id: i32) {
    INJECTING_REMOTE_CLIPBOARD_OWNER.store(conn_id, AtomicOrdering::SeqCst);
}

#[inline]
pub fn take_injecting_remote_clipboard_owner() -> i32 {
    INJECTING_REMOTE_CLIPBOARD_OWNER.swap(0, AtomicOrdering::SeqCst)
}

/// Reset relay state when a session disconnects. If the disconnected session
/// was the clipboard owner or the pending requester, clear the markers so
/// stale requests are not relayed to a dead channel.
pub fn reset_relay_state_on_disconnect(conn_id: i32) {
    if conn_id == 0 {
        return;
    }
    let _ = CLIPBOARD_OWNER.compare_exchange(
        conn_id,
        0,
        AtomicOrdering::SeqCst,
        AtomicOrdering::SeqCst,
    );
    let _ = PENDING_REQUESTER.compare_exchange(
        conn_id,
        0,
        AtomicOrdering::SeqCst,
        AtomicOrdering::SeqCst,
    );
}

impl ClipboardFile {
    pub fn is_stopping_allowed(&self) -> bool {
        matches!(
            self,
            ClipboardFile::MonitorReady
                | ClipboardFile::FormatList { .. }
                | ClipboardFile::FormatDataRequest { .. }
        )
    }

    pub fn is_beginning_message(&self) -> bool {
        matches!(
            self,
            ClipboardFile::MonitorReady | ClipboardFile::FormatList { .. }
        )
    }
}

pub fn get_client_conn_id(peer_id: &str) -> Option<i32> {
    VEC_MSG_CHANNEL
        .read()
        .unwrap()
        .iter()
        .find(|x| x.peer_id == peer_id)
        .map(|x| x.conn_id)
}

fn get_conn_id() -> i32 {
    let mut lock = CLIENT_CONN_ID_COUNTER.lock().unwrap();
    *lock += 1;
    *lock
}

pub fn get_rx_cliprdr_client(
    peer_id: &str,
) -> (i32, Arc<TokioMutex<UnboundedReceiver<ClipboardFile>>>) {
    let mut lock = VEC_MSG_CHANNEL.write().unwrap();
    match lock.iter().find(|x| x.peer_id == peer_id) {
        Some(msg_channel) => (msg_channel.conn_id, msg_channel.receiver.clone()),
        None => {
            let (sender, receiver) = unbounded_channel();
            let receiver = Arc::new(TokioMutex::new(receiver));
            let receiver2 = receiver.clone();
            let conn_id = get_conn_id();
            let msg_channel = MsgChannel {
                peer_id: peer_id.to_owned(),
                conn_id,
                sender,
                receiver,
            };
            lock.push(msg_channel);
            (conn_id, receiver2)
        }
    }
}

pub fn get_rx_cliprdr_server(conn_id: i32) -> Arc<TokioMutex<UnboundedReceiver<ClipboardFile>>> {
    let mut lock = VEC_MSG_CHANNEL.write().unwrap();
    match lock.iter().find(|x| x.conn_id == conn_id) {
        Some(msg_channel) => msg_channel.receiver.clone(),
        None => {
            let (sender, receiver) = unbounded_channel();
            let receiver = Arc::new(TokioMutex::new(receiver));
            let receiver2 = receiver.clone();
            let msg_channel = MsgChannel {
                peer_id: "".to_string(),
                conn_id,
                sender,
                receiver,
            };
            lock.push(msg_channel);
            receiver2
        }
    }
}

pub fn remove_channel_by_conn_id(conn_id: i32) {
    let mut lock = VEC_MSG_CHANNEL.write().unwrap();
    if let Some(index) = lock.iter().position(|x| x.conn_id == conn_id) {
        lock.remove(index);
    }
}

#[cfg(any(target_os = "windows", feature = "unix-file-copy-paste"))]
#[inline]
pub fn send_data(conn_id: i32, data: ClipboardFile) -> Result<(), CliprdrError> {
    #[cfg(target_os = "windows")]
    return send_data_to_channel(conn_id, data);
    #[cfg(not(target_os = "windows"))]
    if conn_id == 0 {
        let _ = send_data_to_all(data);
        Ok(())
    } else {
        send_data_to_channel(conn_id, data)
    }
}

#[inline]
#[cfg(any(target_os = "windows", feature = "unix-file-copy-paste"))]
fn send_data_to_channel(conn_id: i32, data: ClipboardFile) -> Result<(), CliprdrError> {
    if let Some(msg_channel) = VEC_MSG_CHANNEL
        .read()
        .unwrap()
        .iter()
        .find(|x| x.conn_id == conn_id)
    {
        msg_channel
            .sender
            .send(data)
            .map_err(|e| CliprdrError::CommonError {
                description: e.to_string(),
            })
    } else {
        Err(CliprdrError::InvalidRequest {
            description: "conn_id not found".to_string(),
        })
    }
}

#[inline]
#[cfg(target_os = "windows")]
pub fn send_data_exclude(conn_id: i32, data: ClipboardFile) {
    // Need more tests to see if it's necessary to handle the error.
    for msg_channel in VEC_MSG_CHANNEL.read().unwrap().iter() {
        if msg_channel.conn_id != conn_id {
            allow_err!(msg_channel.sender.send(data.clone()));
        }
    }
}

#[inline]
#[cfg(feature = "unix-file-copy-paste")]
fn send_data_to_all(data: ClipboardFile) {
    // Need more tests to see if it's necessary to handle the error.
    for msg_channel in VEC_MSG_CHANNEL.read().unwrap().iter() {
        allow_err!(msg_channel.sender.send(data.clone()));
    }
}

#[cfg(test)]
mod tests {
    // #[test]
    // fn test_cliprdr_run() {
    //     super::cliprdr_run();
    // }
}
