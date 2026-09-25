// src/main.rs
//
// BẢN KHÔNG CẦN DATABASE: toàn bộ dữ liệu sống trong RAM (xem store.rs), để
// bạn chạy thử hết mọi luồng (đăng ký/đăng nhập, check-in GPS, tủ đồ/combo,
// voucher/quét mã, quán/nhân viên/event, search bản đồ) ngay lập tức mà
// không phải cài Postgres. Khi nào cần dữ liệu bền vững thật, xem
// migrations/schema.sql — cấu trúc bảng khớp 1-1 với store.rs.

mod auth;
mod business;
mod jwt;
mod presence;
mod shop;
mod store;

use axum::{
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use store::Store;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let store = Arc::new(Store::seeded());

    let app = Router::new()
        // Auth
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/login", post(auth::login))
        // Check-in (GPS đối tác)
        .route("/v1/checkin", post(handle_checkin))
        // Tủ đồ / Voucher
        .route("/v1/wardrobe", get(shop::get_wardrobe))
        .route("/v1/shop/buy", post(shop::buy_item))
        .route("/v1/shop/equip", post(shop::equip_item))
        .route("/v1/vouchers", get(shop::get_vouchers))
        .route("/v1/vouchers/redeem", post(shop::redeem_voucher))
        // Cơ sở kinh doanh (chủ quán)
        .route("/v1/businesses", post(business::create_business))
        .route("/v1/businesses/mine", get(business::my_businesses))
        .route("/v1/businesses/:id/staff", post(business::add_staff))
        .route("/v1/businesses/:id/staff/:user_id", delete(business::remove_staff))
        .route("/v1/businesses/:id/events", post(business::create_event))
        // Thông báo (khách hàng)
        .route("/v1/notifications", get(business::get_notifications))
        .route("/v1/notifications/:id/read", post(business::mark_notification_read))
        // Bản đồ: danh sách điểm đối tác + tìm kiếm theo tên
        .route("/v1/partners", get(business::list_partners))
        .route("/v1/search", get(business::search_partners))
        // Bản đồ 3D multiplayer: vị trí thời gian thực (polling)
        .route("/v1/presence", get(presence::list_presence).post(presence::update_presence))
        // CORS cho dashboard web (web/owner-dashboard.html) gọi từ trình duyệt.
        // Any() chỉ dùng cho môi trường dev. Lên production phải đổi sang danh sách
        // domain cụ thể, nếu không thì bất kỳ website nào cũng gọi API của bạn được.
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(store);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:44321")
        .await
        .expect("Không mở được cổng 44321");
    tracing::info!("Server (in-memory, không cần DB) chạy tại http://0.0.0.0:44321");
    axum::serve(listener, app).await.unwrap();
}

// ===== Check-in =====

#[derive(Deserialize)]
struct CheckinRequest {
    partner_id: String,
    latitude: f64,
    longitude: f64,
    accuracy_meters: f32,
    #[allow(dead_code)]
    client_timestamp_unix: i64,
}

#[derive(Serialize)]
struct CheckinResponse {
    success: bool,
    points_awarded: i32,
    message: Option<String>,
}

fn json_response(status: StatusCode, body: CheckinResponse) -> (StatusCode, Json<CheckinResponse>) {
    (status, Json(body))
}

async fn handle_checkin(
    axum::extract::State(store): axum::extract::State<Arc<Store>>,
    headers: HeaderMap,
    Json(req): Json<CheckinRequest>,
) -> (StatusCode, Json<CheckinResponse>) {
    let Some(user_id) = jwt::user_id_from_headers(&headers) else {
        return json_response(
            StatusCode::UNAUTHORIZED,
            CheckinResponse { success: false, points_awarded: 0, message: Some("Chưa đăng nhập".into()) },
        );
    };

    let mut data = store.0.write().unwrap();

    let Some(partner) = data.partners.get(&req.partner_id).filter(|p| p.active).cloned() else {
        return json_response(
            StatusCode::NOT_FOUND,
            CheckinResponse { success: false, points_awarded: 0, message: Some("Không tìm thấy đối tác".into()) },
        );
    };

    let distance = haversine_meters(req.latitude, req.longitude, partner.latitude, partner.longitude);
    let accuracy_margin = req.accuracy_meters.clamp(0.0, 50.0) as f64;
    if distance > partner.radius_meters as f64 + accuracy_margin {
        return json_response(
            StatusCode::OK,
            CheckinResponse { success: false, points_awarded: 0, message: Some("Bạn chưa đủ gần đối tác này".into()) },
        );
    }

    let today = Utc::now().date_naive();
    let key = (user_id, req.partner_id.clone(), today);
    if data.checkins.contains(&key) {
        return json_response(
            StatusCode::OK,
            CheckinResponse { success: false, points_awarded: 0, message: Some("Bạn đã check-in đối tác này hôm nay rồi".into()) },
        );
    }
    data.checkins.insert(key);

    if let Some(u) = data.users.get_mut(&user_id) {
        u.points += partner.points_reward;
    }

    json_response(
        StatusCode::OK,
        CheckinResponse { success: true, points_awarded: partner.points_reward, message: None },
    )
}

fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS_M * c
}
