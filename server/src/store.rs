// src/store.rs
//
// Toàn bộ "database" của bản demo này là RAM: một struct Store bọc trong
// Arc<RwLock<..>> để mọi handler axum dùng chung. KHÔNG cần Postgres,
// KHÔNG cần DATABASE_URL, không cần chạy `psql -f schema.sql`.
//
// Đánh đổi: dữ liệu MẤT HẾT mỗi khi restart server (Ctrl+C rồi `cargo run`
// lại là về trạng thái seed ban đầu). Đây là lựa chọn có chủ đích để bạn
// test toàn bộ luồng (quét mã, search map, combo...) ngay lập tức, không bị
// chặn bởi việc cài Postgres. Khi nào sẵn sàng gắn DB thật, xem
// `migrations/schema.sql` — cấu trúc bảng ở đó khớp 1-1 với các struct dưới
// đây, nên việc chuyển sang sqlx/Postgres sau này chỉ là đổi lớp lưu trữ,
// không phải viết lại logic nghiệp vụ.

use chrono::{DateTime, NaiveDate, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub points: i32,
    pub role: String, // "customer" | "staff" | "owner"
}

#[derive(Clone)]
pub struct PartnerLocation {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_meters: f32,
    pub points_reward: i32,
    pub active: bool,
}

#[derive(Clone)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub slot: String,
    pub cost: i32,
}

#[derive(Clone)]
pub struct ComboRule {
    pub id: String,
    pub required_items: Vec<String>,
    pub voucher_title: String,
    pub voucher_validity_days: i64,
    pub business_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct Voucher {
    pub id: Uuid,
    pub user_id: Uuid,
    pub combo_id: String,
    pub code: String,
    pub title: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    pub redeemed_by: Option<Uuid>,
    pub redeemed_at: Option<DateTime<Utc>>,
    pub business_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct Business {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub category: String,
    pub partner_id: Option<String>,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct EventRow {
    pub id: Uuid,
    pub business_id: Uuid,
    pub title: String,
    pub body: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub created_by: Uuid,
}

#[derive(Clone)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    #[allow(dead_code)]
    pub event_id: Option<Uuid>,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct Data {
    pub users: HashMap<Uuid, User>,
    pub partners: HashMap<String, PartnerLocation>,
    /// (user_id, partner_id, ngày) — chặn check-in trùng trong ngày, giống
    /// unique index `idx_one_checkin_per_partner_per_day` trong schema.sql.
    pub checkins: HashSet<(Uuid, String, NaiveDate)>,
    pub items: HashMap<String, Item>,
    /// (user_id, item_id) -> đang mặc hay không
    pub user_items: HashMap<(Uuid, String), bool>,
    pub combos: HashMap<String, ComboRule>,
    /// (user_id, combo_id) đã từng phát voucher chưa — giống
    /// unique index `idx_one_voucher_per_combo`.
    pub issued_combo: HashSet<(Uuid, String)>,
    pub vouchers: HashMap<Uuid, Voucher>,
    pub businesses: HashMap<Uuid, Business>,
    /// (business_id, user_id)
    pub business_staff: HashSet<(Uuid, Uuid)>,
    pub events: HashMap<Uuid, EventRow>,
    pub notifications: HashMap<Uuid, Notification>,
    /// Vị trí gần nhất của từng user trên "bản đồ 3D" — dùng để demo multiplayer
    /// thật (2 tài khoản mở 2 tab, cùng gọi vào 1 server, thấy nhau di chuyển),
    /// không phải bot giả. Không có trong schema.sql vì đây là dữ liệu tức thời,
    /// không cần bền vững qua database thật.
    pub presence: HashMap<Uuid, Presence>,
}

#[derive(Clone)]
pub struct Presence {
    pub latitude: f64,
    pub longitude: f64,
    pub updated_at: DateTime<Utc>,
}

pub struct Store(pub RwLock<Data>);

impl Store {
    pub fn seeded() -> Self {
        let mut d = Data::default();

        // Giống INSERT mẫu trong migrations/schema.sql — toạ độ quanh khu
        // trung tâm Q1 TP.HCM để test luồng GPS check-in và search map cho
        // thật (nhiều điểm, không chỉ 1 quán).
        for p in [
            PartnerLocation { id: "cafe-01".into(), name: "Quán cà phê ABC".into(), latitude: 10.7730, longitude: 106.6985, radius_meters: 25.0, points_reward: 20, active: true },
            PartnerLocation { id: "cafe-02".into(), name: "The Coffee House Bến Thành".into(), latitude: 10.7725, longitude: 106.6980, radius_meters: 30.0, points_reward: 15, active: true },
            PartnerLocation { id: "tra-sua-01".into(), name: "Trà sữa Gong Cha Nguyễn Huệ".into(), latitude: 10.7745, longitude: 106.7040, radius_meters: 25.0, points_reward: 15, active: true },
            PartnerLocation { id: "nhahang-01".into(), name: "Nhà hàng Ngon 138".into(), latitude: 10.7788, longitude: 106.6990, radius_meters: 40.0, points_reward: 30, active: true },
            PartnerLocation { id: "model-shop-01".into(), name: "Tiệm mô hình Anime House".into(), latitude: 10.7760, longitude: 106.6955, radius_meters: 20.0, points_reward: 20, active: true },
        ] {
            d.partners.insert(p.id.clone(), p);
        }

        for it in [
            Item { id: "ao_do".into(), name: "Áo đỏ".into(), slot: "ao".into(), cost: 60 },
            Item { id: "ao_den".into(), name: "Áo đen".into(), slot: "ao".into(), cost: 80 },
            Item { id: "quan_do".into(), name: "Quần đỏ".into(), slot: "quan".into(), cost: 60 },
            Item { id: "quan_den".into(), name: "Quần đen".into(), slot: "quan".into(), cost: 80 },
            Item { id: "non".into(), name: "Nón".into(), slot: "non".into(), cost: 70 },
            Item { id: "giay".into(), name: "Giày".into(), slot: "giay".into(), cost: 90 },
        ] {
            d.items.insert(it.id.clone(), it);
        }

        d.combos.insert(
            "coffee_black".into(),
            ComboRule {
                id: "coffee_black".into(),
                required_items: vec!["ao_den".into(), "quan_den".into(), "non".into(), "giay".into()],
                voucher_title: "Ly cà phê miễn phí".into(),
                voucher_validity_days: 30,
                business_id: None, // gắn sau khi bạn tạo quán, xem README
            },
        );

        Store(RwLock::new(d))
    }
}
