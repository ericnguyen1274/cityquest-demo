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
    extract::Path,
    http::{header, header::HeaderValue, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use store::Store;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();
    if let Err(error) = jwt::validate_configuration() {
        tracing::error!("{error}");
        std::process::exit(1);
    }

    let store = Arc::new(Store::seeded());

    let app = Router::new()
        // Trang web giao diện demo
        .route("/", get(root_handler))
        .route("/:filename", get(serve_html_file))
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
        .layer(
            CorsLayer::new()
                .allow_origin(cors_allowed_origins())
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

async fn root_handler() -> Redirect {
    Redirect::temporary("/simulator.html")
}

async fn serve_html_file(Path(filename): Path<String>) -> Result<Response, StatusCode> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let candidates = [
        PathBuf::from("../web").join(&filename),
        PathBuf::from("web").join(&filename),
    ];
    for path in &candidates {
        if path.is_file() {
            if let Ok(content) = std::fs::read(path) {
                return Ok((
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    content,
                )
                    .into_response());
            }
        }
    }
    Err(StatusCode::NOT_FOUND)
}

fn cors_allowed_origins() -> Vec<HeaderValue> {
    let configured = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_else(|_| {
        "null,http://localhost:8000,http://127.0.0.1:8000".to_string()
    });
    let origins: Vec<HeaderValue> = configured
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| origin.parse::<HeaderValue>().unwrap_or_else(|error| {
            panic!("CORS_ALLOWED_ORIGINS chứa origin không hợp lệ '{origin}': {error}")
        }))
        .collect();
    if origins.is_empty() {
        panic!("CORS_ALLOWED_ORIGINS phải chứa ít nhất một origin");
    }
    origins
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
