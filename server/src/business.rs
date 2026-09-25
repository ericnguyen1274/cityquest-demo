// src/business.rs
//
// Quản lý cơ sở kinh doanh (quán ăn, quán nước, tiệm mô hình...), nhân viên,
// sự kiện, thông báo tới khách hàng — và search (luồng bản đồ). Bản này chạy
// trên in-memory Store (xem store.rs).
//
// Nguyên tắc phân quyền xuyên suốt file này:
//   - Quyền LUÔN tra từ store mỗi request, không tin role nhúng trong JWT.
//   - Quyền gắn với TỪNG QUÁN, không phải toàn hệ thống (staff quán A không
//     được đụng vào dữ liệu quán B).

use crate::jwt::user_id_from_headers;
use crate::store::{Business, Data, EventRow, Notification, Store};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Serialize, Default)]
pub struct ApiMessage {
    pub success: bool,
    pub message: String,
}

fn msg(success: bool, m: &str) -> Json<ApiMessage> {
    Json(ApiMessage { success, message: m.to_string() })
}

// ===== Helper phân quyền =====

/// Người này có phải CHỦ của quán đó không?
fn is_owner_of(data: &Data, user_id: Uuid, business_id: Uuid) -> bool {
    data.businesses.get(&business_id).map(|b| b.owner_id == user_id).unwrap_or(false)
}

/// Người này có được phép thao tác ở quán đó không (chủ HOẶC nhân viên)?
pub fn can_act_for(data: &Data, user_id: Uuid, business_id: Uuid) -> bool {
    is_owner_of(data, user_id, business_id) || data.business_staff.contains(&(business_id, user_id))
}

// ===== POST /v1/businesses — đăng ký quán mới =====

#[derive(Deserialize)]
pub struct CreateBusinessRequest {
    pub name: String,
    pub category: String, // quan_an | quan_nuoc | tiem_mo_hinh | khac
    pub partner_id: Option<String>,
    /// Tuỳ chọn: gắn luôn 1 combo có sẵn (vd "coffee_black") vào quán này, để
    /// voucher sinh ra từ combo đó redeem được ngay. Thay cho việc phải chạy
    /// `UPDATE combo_rules SET business_id=...` bằng SQL như bản có Postgres —
    /// ở bản in-memory này không có psql để gõ lệnh đó.
    pub combo_id: Option<String>,
}

#[derive(Serialize, Default)]
pub struct CreateBusinessResponse {
    pub success: bool,
    pub business_id: Option<Uuid>,
    pub message: String,
}

pub async fn create_business(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<CreateBusinessRequest>,
) -> (StatusCode, Json<CreateBusinessResponse>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(CreateBusinessResponse { message: "Chưa đăng nhập".into(), ..Default::default() }));
    };

    const VALID: [&str; 4] = ["quan_an", "quan_nuoc", "tiem_mo_hinh", "khac"];
    if !VALID.contains(&req.category.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(CreateBusinessResponse { message: format!("Loại hình không hợp lệ. Chọn một trong: {}", VALID.join(", ")), ..Default::default() }),
        );
    }
    if req.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(CreateBusinessResponse { message: "Tên quán không được để trống".into(), ..Default::default() }));
    }

    let mut data = store.0.write().unwrap();

    if let Some(pid) = &req.partner_id {
        if !data.partners.contains_key(pid) {
            return (StatusCode::BAD_REQUEST, Json(CreateBusinessResponse { message: format!("Không tìm thấy partner_id '{}' — xem danh sách tại GET /v1/partners", pid), ..Default::default() }));
        }
    }
    if let Some(cid) = &req.combo_id {
        if !data.combos.contains_key(cid) {
            return (StatusCode::BAD_REQUEST, Json(CreateBusinessResponse { message: format!("Không tìm thấy combo_id '{}'", cid), ..Default::default() }));
        }
    }

    let id = Uuid::new_v4();
    data.businesses.insert(
        id,
        Business { id, owner_id: user_id, name: req.name.trim().to_string(), category: req.category.clone(), partner_id: req.partner_id.clone() },
    );

    if let Some(cid) = &req.combo_id {
        if let Some(combo) = data.combos.get_mut(cid) {
            combo.business_id = Some(id);
        }
        // Voucher nào đã phát ra từ combo này trước đó (business_id = None) cũng
        // được gắn lại vào quán vừa tạo, để không bị kẹt "chưa gắn quán nào".
        for v in data.vouchers.values_mut() {
            if &v.combo_id == cid && v.business_id.is_none() {
                v.business_id = Some(id);
            }
        }
    }

    // Người tạo quán trở thành 'owner'. Không hạ cấp ai đang là owner sẵn.
    if let Some(u) = data.users.get_mut(&user_id) {
        if u.role == "customer" {
            u.role = "owner".to_string();
        }
    }

    (StatusCode::CREATED, Json(CreateBusinessResponse { success: true, business_id: Some(id), message: "Tạo quán thành công".into() }))
}

// ===== POST /v1/businesses/:id/staff — chủ quán thêm nhân viên =====

#[derive(Deserialize)]
pub struct AddStaffRequest {
    pub email: String,
}

pub async fn add_staff(
    State(store): State<Arc<Store>>,
    Path(business_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<AddStaffRequest>,
) -> (StatusCode, Json<ApiMessage>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, msg(false, "Chưa đăng nhập"));
    };

    let mut data = store.0.write().unwrap();

    if !is_owner_of(&data, user_id, business_id) {
        return (StatusCode::FORBIDDEN, msg(false, "Chỉ chủ quán mới được thêm nhân viên"));
    }

    let email = req.email.trim().to_lowercase();
    let Some(target_id) = data.users.values().find(|u| u.email == email).map(|u| u.id) else {
        return (StatusCode::NOT_FOUND, msg(false, "Không tìm thấy tài khoản với email này"));
    };

    if !data.business_staff.insert((business_id, target_id)) {
        return (StatusCode::OK, msg(false, "Người này đã là nhân viên của quán rồi"));
    }

    if let Some(u) = data.users.get_mut(&target_id) {
        if u.role == "customer" {
            u.role = "staff".to_string();
        }
    }

    (StatusCode::CREATED, msg(true, "Đã thêm nhân viên"))
}

// ===== DELETE /v1/businesses/:id/staff/:user_id — gỡ nhân viên =====

pub async fn remove_staff(
    State(store): State<Arc<Store>>,
    Path((business_id, staff_user_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> (StatusCode, Json<ApiMessage>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, msg(false, "Chưa đăng nhập"));
    };

    let mut data = store.0.write().unwrap();

    if !is_owner_of(&data, user_id, business_id) {
        return (StatusCode::FORBIDDEN, msg(false, "Chỉ chủ quán mới được gỡ nhân viên"));
    }

    if !data.business_staff.remove(&(business_id, staff_user_id)) {
        return (StatusCode::NOT_FOUND, msg(false, "Người này không phải nhân viên của quán"));
    }

    // Nếu không còn làm ở quán nào nữa thì trả về khách hàng thường.
    let still_staff = data.business_staff.iter().any(|(_, uid)| *uid == staff_user_id);
    let owns_any = data.businesses.values().any(|b| b.owner_id == staff_user_id);
    if !still_staff && !owns_any {
        if let Some(u) = data.users.get_mut(&staff_user_id) {
            if u.role == "staff" {
                u.role = "customer".to_string();
            }
        }
    }

    (StatusCode::OK, msg(true, "Đã gỡ nhân viên"))
}

// ===== POST /v1/businesses/:id/events — tạo event + tự động gửi thông báo =====

#[derive(Deserialize)]
pub struct CreateEventRequest {
    pub title: String,
    pub body: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
}

#[derive(Serialize, Default)]
pub struct CreateEventResponse {
    pub success: bool,
    pub event_id: Option<Uuid>,
    pub notified_customers: i64,
    pub message: String,
}

pub async fn create_event(
    State(store): State<Arc<Store>>,
    Path(business_id): Path<Uuid>,
    headers: HeaderMap,
    Json(req): Json<CreateEventRequest>,
) -> (StatusCode, Json<CreateEventResponse>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(CreateEventResponse { message: "Chưa đăng nhập".into(), ..Default::default() }));
    };

    let mut data = store.0.write().unwrap();

    if !can_act_for(&data, user_id, business_id) {
        return (StatusCode::FORBIDDEN, Json(CreateEventResponse { message: "Bạn không có quyền tạo sự kiện cho quán này".into(), ..Default::default() }));
    }
    if req.ends_at <= req.starts_at {
        return (StatusCode::BAD_REQUEST, Json(CreateEventResponse { message: "Thời gian kết thúc phải sau thời gian bắt đầu".into(), ..Default::default() }));
    }

    let Some(business) = data.businesses.get(&business_id).cloned() else {
        return (StatusCode::NOT_FOUND, Json(CreateEventResponse { message: "Không tìm thấy quán".into(), ..Default::default() }));
    };

    let event_id = Uuid::new_v4();
    data.events.insert(
        event_id,
        EventRow { id: event_id, business_id, title: req.title.trim().to_string(), body: req.body.trim().to_string(), starts_at: req.starts_at, ends_at: req.ends_at, created_by: user_id },
    );

    // Gửi cho những khách đã từng check-in tại partner_id gắn với quán này.
    let mut notified = 0i64;
    if let Some(partner_id) = &business.partner_id {
        let customer_ids: std::collections::HashSet<Uuid> =
            data.checkins.iter().filter(|(_, pid, _)| pid == partner_id).map(|(uid, _, _)| *uid).collect();

        for uid in customer_ids {
            let nid = Uuid::new_v4();
            data.notifications.insert(
                nid,
                Notification { id: nid, user_id: uid, event_id: Some(event_id), title: req.title.trim().to_string(), body: req.body.trim().to_string(), read: false, created_at: Utc::now() },
            );
            notified += 1;
        }
    }

    (
        StatusCode::CREATED,
        Json(CreateEventResponse { success: true, event_id: Some(event_id), notified_customers: notified, message: format!("Đã tạo sự kiện và gửi thông báo tới {} khách hàng", notified) }),
    )
}

// ===== GET /v1/notifications — khách hàng xem thông báo của mình =====

#[derive(Serialize)]
pub struct NotificationDto {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

pub async fn get_notifications(State(store): State<Arc<Store>>, headers: HeaderMap) -> (StatusCode, Json<Vec<NotificationDto>>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(vec![]));
    };

    let data = store.0.read().unwrap();
    let mut list: Vec<NotificationDto> = data
        .notifications
        .values()
        .filter(|n| n.user_id == user_id)
        .map(|n| NotificationDto { id: n.id, title: n.title.clone(), body: n.body.clone(), read: n.read, created_at: n.created_at })
        .collect();
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list.truncate(50);

    (StatusCode::OK, Json(list))
}

// ===== POST /v1/notifications/:id/read — đánh dấu đã đọc =====

pub async fn mark_notification_read(State(store): State<Arc<Store>>, Path(notification_id): Path<Uuid>, headers: HeaderMap) -> (StatusCode, Json<ApiMessage>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, msg(false, "Chưa đăng nhập"));
    };

    let mut data = store.0.write().unwrap();
    match data.notifications.get_mut(&notification_id) {
        Some(n) if n.user_id == user_id => {
            n.read = true;
            (StatusCode::OK, msg(true, "Đã đánh dấu đã đọc"))
        }
        _ => (StatusCode::NOT_FOUND, msg(false, "Không tìm thấy thông báo")),
    }
}

// ===== GET /v1/businesses/mine — danh sách quán tôi quản lý =====

#[derive(Serialize)]
pub struct BusinessDto {
    pub id: Uuid,
    pub name: String,
    pub category: String,
    pub partner_id: Option<String>,
    pub my_role: String, // 'owner' | 'staff'
}

pub async fn my_businesses(State(store): State<Arc<Store>>, headers: HeaderMap) -> (StatusCode, Json<Vec<BusinessDto>>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(vec![]));
    };

    let data = store.0.read().unwrap();
    let mut list: Vec<BusinessDto> = Vec::new();
    for b in data.businesses.values() {
        if b.owner_id == user_id {
            list.push(BusinessDto { id: b.id, name: b.name.clone(), category: b.category.clone(), partner_id: b.partner_id.clone(), my_role: "owner".into() });
        } else if data.business_staff.contains(&(b.id, user_id)) {
            list.push(BusinessDto { id: b.id, name: b.name.clone(), category: b.category.clone(), partner_id: b.partner_id.clone(), my_role: "staff".into() });
        }
    }

    (StatusCode::OK, Json(list))
}

// ===== Luồng bản đồ: GET /v1/partners và GET /v1/search =====
//
// OpenFreeMap chỉ phục vụ vector tile, không có API search/geocode, nên phần
// search tự làm trên chính dữ liệu partner_locations/businesses trong store
// (README bản cũ liệt kê đây là "yêu cầu số 7 — chưa làm").

#[derive(Serialize)]
pub struct PartnerDto {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub points_reward: i32,
}

/// GET /v1/partners — toàn bộ điểm đối tác đang active, để vẽ pin lên bản đồ.
pub async fn list_partners(State(store): State<Arc<Store>>) -> Json<Vec<PartnerDto>> {
    let data = store.0.read().unwrap();
    let list = data
        .partners
        .values()
        .filter(|p| p.active)
        .map(|p| PartnerDto { id: p.id.clone(), name: p.name.clone(), latitude: p.latitude, longitude: p.longitude, points_reward: p.points_reward })
        .collect();
    Json(list)
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub partner_id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Có quán (business) nào của app gắn với điểm này chưa, hay chỉ là điểm GPS trần.
    pub business_id: Option<Uuid>,
}

/// GET /v1/search?q=... — tìm quán theo tên (không phân biệt hoa/thường, khớp
/// một phần), dùng cho ô tìm kiếm trên bản đồ trong app/Unity.
pub async fn search_partners(State(store): State<Arc<Store>>, Query(params): Query<SearchQuery>) -> Json<Vec<SearchResult>> {
    let q = params.q.trim().to_lowercase();
    let data = store.0.read().unwrap();

    let mut results: Vec<SearchResult> = data
        .partners
        .values()
        .filter(|p| p.active && (q.is_empty() || p.name.to_lowercase().contains(&q) || p.id.to_lowercase().contains(&q)))
        .map(|p| {
            let business_id = data.businesses.values().find(|b| b.partner_id.as_deref() == Some(p.id.as_str())).map(|b| b.id);
            SearchResult { partner_id: p.id.clone(), name: p.name.clone(), latitude: p.latitude, longitude: p.longitude, business_id }
        })
        .collect();

    results.sort_by(|a, b| a.name.cmp(&b.name));
    Json(results)
}
