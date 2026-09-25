// src/shop.rs
//
// Toàn bộ logic Tủ đồ: mua đồ, mặc đồ, và tự động phát hiện + phát voucher khi
// đúng combo. Bản này chạy trên in-memory Store (xem store.rs).

use crate::jwt::user_id_from_headers;
use crate::store::{Store, Voucher};
use axum::{extract::State, http::HeaderMap, http::StatusCode, Json};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

fn unauthorized<T: Default + Serialize>() -> (StatusCode, Json<T>) {
    (StatusCode::UNAUTHORIZED, Json(T::default()))
}

// ----- GET /v1/wardrobe -----

#[derive(Serialize, Default)]
pub struct WardrobeResponse {
    pub points: i32,
    pub items: Vec<ItemStatus>,
}

#[derive(Serialize)]
pub struct ItemStatus {
    pub id: String,
    pub name: String,
    pub slot: String,
    pub cost: i32,
    pub owned: bool,
    pub equipped: bool,
}

pub async fn get_wardrobe(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
) -> Result<Json<WardrobeResponse>, StatusCode> {
    let user_id = user_id_from_headers(&headers).ok_or(StatusCode::UNAUTHORIZED)?;

    let data = store.0.read().unwrap();
    let points = data.users.get(&user_id).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?.points;

    let mut items: Vec<ItemStatus> = data
        .items
        .values()
        .map(|i| {
            let equipped = *data.user_items.get(&(user_id, i.id.clone())).unwrap_or(&false);
            let owned = data.user_items.contains_key(&(user_id, i.id.clone()));
            ItemStatus { id: i.id.clone(), name: i.name.clone(), slot: i.slot.clone(), cost: i.cost, owned, equipped }
        })
        .collect();
    items.sort_by(|a, b| a.slot.cmp(&b.slot).then(a.cost.cmp(&b.cost)));

    Ok(Json(WardrobeResponse { points, items }))
}

// ----- POST /v1/shop/buy -----

#[derive(Deserialize)]
pub struct BuyRequest {
    pub item_id: String,
}

#[derive(Serialize, Default)]
pub struct ShopActionResponse {
    pub success: bool,
    pub points: i32,
    pub combo_unlocked: Option<ComboUnlocked>,
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct ComboUnlocked {
    pub voucher_code: String,
    pub voucher_title: String,
}

pub async fn buy_item(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<BuyRequest>,
) -> (StatusCode, Json<ShopActionResponse>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return unauthorized();
    };

    let mut data = store.0.write().unwrap();

    let Some(item) = data.items.get(&req.item_id).cloned() else {
        return (StatusCode::NOT_FOUND, Json(ShopActionResponse { success: false, points: 0, combo_unlocked: None, message: Some("Không tìm thấy vật phẩm".into()) }));
    };

    let user_points = data.users.get(&user_id).map(|u| u.points).unwrap_or(0);
    if user_points < item.cost {
        return (StatusCode::OK, Json(ShopActionResponse { success: false, points: user_points, combo_unlocked: None, message: Some("Không đủ điểm".into()) }));
    }

    // Trừ điểm, bỏ trang bị các item cùng slot (chỉ mặc 1 item / slot), rồi thêm item mới ở trạng thái đang mặc.
    if let Some(u) = data.users.get_mut(&user_id) {
        u.points -= item.cost;
    }
    unequip_slot(&mut data, user_id, &item.slot);
    data.user_items.insert((user_id, req.item_id.clone()), true);

    let combo_unlocked = check_and_grant_combo(&mut data, user_id);
    let new_points = user_points - item.cost;

    (StatusCode::OK, Json(ShopActionResponse { success: true, points: new_points, combo_unlocked, message: None }))
}

// ----- POST /v1/shop/equip (đổi màu giữa các item đã sở hữu cùng slot, không tốn điểm) -----

#[derive(Deserialize)]
pub struct EquipRequest {
    pub item_id: String,
}

pub async fn equip_item(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<EquipRequest>,
) -> (StatusCode, Json<ShopActionResponse>) {
    let Some(user_id) = user_id_from_headers(&headers) else {
        return unauthorized();
    };

    let mut data = store.0.write().unwrap();

    if !data.user_items.contains_key(&(user_id, req.item_id.clone())) {
        return (StatusCode::BAD_REQUEST, Json(ShopActionResponse { success: false, points: 0, combo_unlocked: None, message: Some("Bạn chưa sở hữu vật phẩm này".into()) }));
    }

    let Some(slot) = data.items.get(&req.item_id).map(|i| i.slot.clone()) else {
        return (StatusCode::NOT_FOUND, Json(ShopActionResponse { success: false, points: 0, combo_unlocked: None, message: Some("Không tìm thấy vật phẩm".into()) }));
    };

    unequip_slot(&mut data, user_id, &slot);
    data.user_items.insert((user_id, req.item_id.clone()), true);

    let combo_unlocked = check_and_grant_combo(&mut data, user_id);
    let points = data.users.get(&user_id).map(|u| u.points).unwrap_or(0);

    (StatusCode::OK, Json(ShopActionResponse { success: true, points, combo_unlocked, message: None }))
}

/// Bỏ trang bị mọi item user đang sở hữu thuộc slot này (chỉ mặc 1 item / slot).
fn unequip_slot(data: &mut crate::store::Data, user_id: Uuid, slot: &str) {
    let item_ids_in_slot: Vec<String> =
        data.items.values().filter(|i| i.slot == slot).map(|i| i.id.clone()).collect();
    for item_id in item_ids_in_slot {
        if let Some(equipped) = data.user_items.get_mut(&(user_id, item_id)) {
            *equipped = false;
        }
    }
}

/// Kiểm tra toàn bộ combo_rules, so với tập item đang equip của user.
/// Nếu khớp và user chưa từng nhận voucher của combo đó, tạo voucher mới.
fn check_and_grant_combo(data: &mut crate::store::Data, user_id: Uuid) -> Option<ComboUnlocked> {
    let equipped: Vec<String> = data
        .user_items
        .iter()
        .filter(|((uid, _), eq)| *uid == user_id && **eq)
        .map(|((_, item_id), _)| item_id.clone())
        .collect();

    let combos: Vec<_> = data.combos.values().cloned().collect();

    for combo in combos {
        let all_equipped = combo.required_items.iter().all(|req_item| equipped.contains(req_item));
        if !all_equipped {
            continue;
        }
        if data.issued_combo.contains(&(user_id, combo.id.clone())) {
            continue;
        }

        let mut rng = rand::thread_rng();
        let code = format!("CFE-{:04}-{}", rng.gen_range(0..10000), combo.id.to_uppercase());
        let expires_at = chrono::Utc::now() + chrono::Duration::days(combo.voucher_validity_days);

        let voucher_id = Uuid::new_v4();
        data.vouchers.insert(
            voucher_id,
            Voucher {
                id: voucher_id,
                user_id,
                combo_id: combo.id.clone(),
                code: code.clone(),
                title: combo.voucher_title.clone(),
                expires_at,
                used: false,
                redeemed_by: None,
                redeemed_at: None,
                business_id: combo.business_id,
            },
        );
        data.issued_combo.insert((user_id, combo.id.clone()));

        return Some(ComboUnlocked { voucher_code: code, voucher_title: combo.voucher_title });
    }

    None
}

// ----- GET /v1/vouchers -----

#[derive(Serialize)]
pub struct VoucherItem {
    pub code: String,
    pub title: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub used: bool,
}

pub async fn get_vouchers(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
) -> Result<Json<Vec<VoucherItem>>, StatusCode> {
    let user_id = user_id_from_headers(&headers).ok_or(StatusCode::UNAUTHORIZED)?;

    let data = store.0.read().unwrap();
    let mut list: Vec<&Voucher> = data.vouchers.values().filter(|v| v.user_id == user_id).collect();
    list.sort_by(|a, b| b.expires_at.cmp(&a.expires_at));

    Ok(Json(
        list.into_iter()
            .map(|v| VoucherItem { code: v.code.clone(), title: v.title.clone(), expires_at: v.expires_at, used: v.used })
            .collect(),
    ))
}

// ----- POST /v1/vouchers/redeem (nhân viên quán quét/nhập mã) -----
//
// Bắt buộc:
//   1. Phải có JWT hợp lệ (đăng nhập).
//   2. Tài khoản đó phải có role = 'staff' hoặc 'owner' CỦA ĐÚNG QUÁN phát hành
//      voucher (tra qua business_staff/businesses trong store, không tin role
//      nhúng sẵn trong JWT).
//   3. Ghi lại redeemed_by + redeemed_at để có dấu vết khi cần tra soát.

#[derive(Deserialize)]
pub struct RedeemRequest {
    pub code: String,
}

#[derive(Serialize)]
pub struct RedeemResponse {
    pub success: bool,
    pub message: String,
}

fn redeem_err(msg: &str) -> Json<RedeemResponse> {
    Json(RedeemResponse { success: false, message: msg.to_string() })
}

pub async fn redeem_voucher(
    State(store): State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<RedeemRequest>,
) -> (StatusCode, Json<RedeemResponse>) {
    let Some(staff_id) = user_id_from_headers(&headers) else {
        return (StatusCode::UNAUTHORIZED, redeem_err("Chưa đăng nhập"));
    };

    let mut data = store.0.write().unwrap();

    let Some(voucher_id) = data.vouchers.values().find(|v| v.code == req.code).map(|v| v.id) else {
        return (StatusCode::OK, redeem_err("Mã không hợp lệ"));
    };

    let voucher = data.vouchers.get(&voucher_id).unwrap().clone();

    if voucher.used {
        return (StatusCode::OK, redeem_err("Mã này đã được sử dụng"));
    }
    if voucher.expires_at <= chrono::Utc::now() {
        return (StatusCode::OK, redeem_err("Mã đã hết hạn"));
    }

    let Some(business_id) = voucher.business_id else {
        return (
            StatusCode::CONFLICT,
            redeem_err("Voucher này chưa gắn với quán nào — liên hệ quản trị viên"),
        );
    };

    if !crate::business::can_act_for(&data, staff_id, business_id) {
        return (
            StatusCode::FORBIDDEN,
            redeem_err("Bạn không có quyền đổi voucher của quán này"),
        );
    }

    // Kiểm tra used = false lại lần nữa ngay trước khi ghi, để nếu 2 request
    // cùng lúc quét 1 mã thì chỉ 1 cái thắng (không có race condition thật sự
    // với RwLock write-lock, nhưng giữ đúng thứ tự logic gốc).
    let v = data.vouchers.get_mut(&voucher_id).unwrap();
    if v.used {
        return (StatusCode::OK, redeem_err("Mã vừa được sử dụng bởi người khác"));
    }
    v.used = true;
    v.redeemed_by = Some(staff_id);
    v.redeemed_at = Some(chrono::Utc::now());

    (StatusCode::OK, Json(RedeemResponse { success: true, message: "Đổi quà thành công".into() }))
}
