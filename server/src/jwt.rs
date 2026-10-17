// src/jwt.rs
//
// Tạo và xác thực JWT. Thay thế cho việc nhận X-Debug-User-Id thẳng như bản trước.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEV_SECRET: &str = "dev-only-insecure-secret-doi-truoc-khi-deploy";

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String, // user_id dạng string
    exp: usize,  // hết hạn (unix timestamp)
}

fn secret() -> String {
    // Nếu chưa tạo .env / chưa set JWT_SECRET, dùng giá trị mặc định cho DEV
    // để server chạy được ngay. Trước khi deploy thật, PHẢI đặt JWT_SECRET
    // riêng (xem .env.example) — nếu không ai cũng đoán được secret và tự
    // ký JWT giả mạo bất kỳ user_id nào.
    std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| DEV_SECRET.to_string())
}

pub fn validate_configuration() -> Result<(), String> {
    if !std::env::var("APP_ENV").unwrap_or_default().eq_ignore_ascii_case("production") {
        return Ok(());
    }

    let configured_secret = std::env::var("JWT_SECRET")
        .map_err(|_| "JWT_SECRET bắt buộc phải được cấu hình trong production".to_string())?;
    if configured_secret.len() < 32 || configured_secret == DEV_SECRET {
        return Err("JWT_SECRET trong production phải dài ít nhất 32 ký tự và không được dùng secret dev".into());
    }
    if std::env::var("CORS_ALLOWED_ORIGINS").map_or(true, |origins| origins.trim().is_empty()) {
        return Err("CORS_ALLOWED_ORIGINS bắt buộc phải được cấu hình trong production".into());
    }
    Ok(())
}

pub fn create_token(user_id: Uuid) -> String {
    let exp = (chrono::Utc::now() + chrono::Duration::days(30)).timestamp() as usize;
    let claims = Claims { sub: user_id.to_string(), exp };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret().as_bytes()))
        .expect("Không tạo được JWT")
}

pub fn verify_token(token: &str) -> Option<Uuid> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret().as_bytes()),
        &Validation::default(),
    )
    .ok()?;
    Uuid::parse_str(&data.claims.sub).ok()
}

/// Đọc header "Authorization: Bearer <token>" và trả về user_id nếu hợp lệ.
pub fn user_id_from_headers(headers: &axum::http::HeaderMap) -> Option<Uuid> {
    let auth = headers.get("Authorization")?.to_str().ok()?;
    let token = auth.strip_prefix("Bearer ")?;
    verify_token(token)
}
