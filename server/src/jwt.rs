// src/jwt.rs
//
// Tạo và xác thực JWT. Thay thế cho việc nhận X-Debug-User-Id thẳng như bản trước.

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
        .unwrap_or_else(|_| "dev-only-insecure-secret-doi-truoc-khi-deploy".to_string())
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
