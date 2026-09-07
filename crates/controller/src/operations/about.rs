use axum::Json;

use crate::domain::AboutResponse;

const PRODUCT_NAME: &str = "Videnoa Controller";
const SOURCE_URL: &str = "https://github.com/ControlNet/videnoa";

/// Reports the build identity of this Controller.
///
/// Authenticated on purpose. `/api/health` answers without a session so that a
/// probe can confirm the port is alive; a precise build fingerprint is a
/// different thing and belongs behind the same gate as the rest of the console.
pub(super) async fn get() -> Json<AboutResponse> {
    Json(AboutResponse {
        name: PRODUCT_NAME.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        source_url: SOURCE_URL.to_owned(),
    })
}
