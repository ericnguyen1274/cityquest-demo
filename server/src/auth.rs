// src/auth.rs
//
// POST /v1/auth/register và POST /v1/auth/login — sinh JWT dùng cho mọi request sau đó
// (client gắn vào header "Authorization: Bearer <token>").
//
// Bản này lưu user trong RAM (xem store.rs) thay vì Postgres, để chạy demo
// ngay không cần cài DB.

use crate::jwt::create_token;
use crate::store::{Store, User};
use axum::{extract::State, http::StatusCode, Json};
use bcrypt::{hash, verify, DEFAULT_COST};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub success: bool,
    pub token: Option<String>,
    pub message: Option<String>,
}

fn ok(token: String) -> (StatusCode, Json<AuthResponse>) {
    (StatusCode::OK, Json(AuthResponse { success: true, token: Some(token), message: None }))
}

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<AuthResponse>) {
    (status, Json(AuthResponse { success: false, token: None, message: Some(msg.to_string()) }))
}

pub async fn register(
    State(store): State<Arc<Store>>,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<AuthResponse>) {
    if req.password.len() < 6 {
        return err(StatusCode::BAD_REQUEST, "Mật khẩu tối thiểu 6 ký tự");
    }
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return err(StatusCode::BAD_REQUEST, "Email không được để trống");
    }

    let password_hash = hash(&req.password, DEFAULT_COST).expect("Lỗi hash mật khẩu");

    let mut data = store.0.write().unwrap();
    if data.users.values().any(|u| u.email == email) {
        return err(StatusCode::CONFLICT, "Email đã được đăng ký");
    }

    let id = Uuid::new_v4();
    data.users.insert(
        id,
        User {
            id,
            email,
            password_hash,
            display_name: req.display_name.trim().to_string(),
            // DEV-ONLY: tặng sẵn điểm khi đăng ký để test luồng tủ đồ/combo
            // ngay không cần check-in nhiều lần chờ đủ điểm (320 = tổng giá
            // đúng 4 món của combo "coffee_black" mẫu). Trước đây bản có
            // Postgres test bằng cách gõ SQL sửa điểm thẳng trong DB — bản
            // in-memory không có psql nên bù bằng cách này. XOÁ dòng cộng
            // điểm này (đặt lại `points: 0`) khi không cần nữa.
            points: 400,
            role: "customer".to_string(),
        },
    );
    drop(data);

    ok(create_token(id))
}

pub async fn login(
    State(store): State<Arc<Store>>,
    Json(req): Json<LoginRequest>,
) -> (StatusCode, Json<AuthResponse>) {
    let email = req.email.trim().to_lowercase();
    let data = store.0.read().unwrap();
    let user = data.users.values().find(|u| u.email == email);

    let Some(user) = user else {
        return err(StatusCode::UNAUTHORIZED, "Email hoặc mật khẩu không đúng");
    };

    let valid = verify(&req.password, &user.password_hash).unwrap_or(false);
    if !valid {
        return err(StatusCode::UNAUTHORIZED, "Email hoặc mật khẩu không đúng");
    }

    ok(create_token(user.id))
}
