// src/presence.rs
//
// "Ai đang ở đâu" trên bản đồ 3D — để demo multiplayer THẬT: mở app giả lập ở
// 2 tab trình duyệt, đăng nhập 2 tài khoản khác nhau, cả hai cùng POST vị trí
// của mình lên đây và GET vị trí của người khác, thấy nhau di chuyển. Không
// dùng WebSocket (để giữ server đơn giản) — client tự polling mỗi 1-2 giây.
//
// Một entry hết hạn sau PRESENCE_TTL_SECS không cập nhật (người đó coi như đã
// thoát app) — tự "biến mất" khỏi bản đồ của người khác mà không cần API logout riêng.

use crate::jwt::user_id_from_headers;
use crate::store::{Presence, Store};
use axum::{extract::State, http::HeaderMap, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

const PRESENCE_TTL_SECS: i64 = 120;

#[derive(Deserialize)]
pub struct UpdatePresenceRequest {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Serialize, Default)]
pub struct ApiMessage {
    pub success: bool,
    pub message: String,
}

pub async fn update_presence(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<UpdatePresenceRequest>,
) -> (StatusCode, Json<ApiMessage>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(ApiMessage { message: "Chưa đăng nhập".into(), ..Default::default() }));
    };

    let mut data = store.0.write().unwrap();
    data.presence.insert(user_id, Presence { latitude: req.latitude, longitude: req.longitude, updated_at: Utc::now() });

    (StatusCode::OK, Json(ApiMessage { success: true, message: "ok".into() }))
}

#[derive(Serialize)]
pub struct Outfit {
    pub ao: Option<String>,
    pub quan: Option<String>,
    pub non: Option<String>,
    pub giay: Option<String>,
}

#[derive(Serialize)]
pub struct PlayerDto {
    pub user_id: Uuid,
    pub display_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub outfit: Outfit,
}

/// GET /v1/presence — toàn bộ user đang "online" (đã POST vị trí trong
/// PRESENCE_TTL_SECS giây gần nhất), kèm luôn đồ đang mặc để avatar hiển thị
/// đúng — client không cần gọi thêm /v1/wardrobe cho từng người.
pub async fn list_presence(State(store): State<Arc<Store>>) -> Json<Vec<PlayerDto>> {
    let data = store.0.read().unwrap();
    let now = Utc::now();

    let mut list: Vec<PlayerDto> = data
        .presence
        .iter()
        .filter(|(_, p)| (now - p.updated_at).num_seconds() <= PRESENCE_TTL_SECS)
        .filter_map(|(uid, p)| {
            let user = data.users.get(uid)?;

            let mut outfit = Outfit { ao: None, quan: None, non: None, giay: None };
            for ((u, item_id), equipped) in data.user_items.iter() {
                if u != uid || !*equipped {
                    continue;
                }
                if let Some(item) = data.items.get(item_id) {
                    match item.slot.as_str() {
                        "ao" => outfit.ao = Some(item_id.clone()),
                        "quan" => outfit.quan = Some(item_id.clone()),
                        "non" => outfit.non = Some(item_id.clone()),
                        "giay" => outfit.giay = Some(item_id.clone()),
                        _ => {}
                    }
                }
            }

            Some(PlayerDto { user_id: *uid, display_name: user.display_name.clone(), latitude: p.latitude, longitude: p.longitude, outfit })
        })
        .collect();

    list.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Json(list)
}
