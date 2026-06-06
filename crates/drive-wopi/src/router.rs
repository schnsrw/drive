//! WOPI router fragment. Mounted at `/wopi` on the app origin by `drive-http`.

use axum::{routing::get, Router};

use crate::handlers::{check_file_info, get_file, lock_dispatch, put_file, WopiAppState};

#[must_use]
pub fn router(state: WopiAppState) -> Router {
    Router::new()
        .route(
            "/wopi/files/{file_id}",
            get(check_file_info).post(lock_dispatch),
        )
        .route(
            "/wopi/files/{file_id}/contents",
            get(get_file).post(put_file),
        )
        .with_state(state)
}
