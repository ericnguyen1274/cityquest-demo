# CityQuest Demo — Bản chạy ngay, không cần DB

Toàn bộ code cần thiết cho MVP: đăng nhập, bản đồ GPS check-in + tìm kiếm, tủ đồ + combo + voucher,
quán/nhân viên/event.

**Bản này lưu dữ liệu trong RAM (in-memory)** — không cần cài Postgres, không cần chạy `psql`,
không cần `DATABASE_URL`. Mục tiêu: chạy thử được TOÀN BỘ luồng ngay (quét mã, search bản đồ,
combo...) trước, setup database thật sau khi các luồng đã ổn.

⚠️ **Đánh đổi:** dữ liệu (user, điểm, voucher, quán...) **mất hết mỗi khi restart server**
(Ctrl+C rồi `cargo run` lại là về trạng thái seed ban đầu). Khi nào cần dữ liệu bền vững, xem
mục "Chuyển sang Postgres" ở cuối file.

```
cityquest-demo/
├── unity/Assets/Scripts/
│   ├── AuthManager.cs        # đăng ký / đăng nhập, lưu JWT
│   ├── SessionManager.cs     # giữ JWT dùng chung cho mọi request
│   ├── PartnerCheckin.cs     # check-in theo GPS thật (Input.location của Unity)
│   └── WardrobeManager.cs    # mua/mặc đồ, nhận voucher khi đúng combo
└── server/
    ├── Cargo.toml
    ├── .env.example
    ├── migrations/schema.sql # cấu trúc DB để dùng SAU, khi chuyển sang Postgres
    └── src/
        ├── main.rs           # router chính + endpoint /v1/checkin
        ├── auth.rs           # /v1/auth/register, /v1/auth/login
        ├── jwt.rs            # tạo & xác thực JWT dùng chung
        ├── shop.rs           # /v1/wardrobe, /v1/shop/buy, /v1/shop/equip, /v1/vouchers
        ├── business.rs       # /v1/businesses, staff, events, notifications, search/partners
        └── store.rs          # "database" in-memory — toàn bộ state sống trong RAM
```

## Chạy server (không cần cài gì thêm ngoài Rust)

```bash
cd server
cp .env.example .env        # tuỳ chọn — không có cũng chạy được (xem ghi chú JWT_SECRET)
cargo run
```

Server chạy ở `http://localhost:44321`, có sẵn dữ liệu mẫu: 5 điểm đối tác quanh khu trung tâm
TP.HCM (`cafe-01`, `cafe-02`, `tra-sua-01`, `nhahang-01`, `model-shop-01`), 6 món đồ, 1 combo
mẫu `coffee_black`.

Khi mở web từ `file://`, origin là `null` và đã nằm trong CORS mặc định cho demo. Nếu phục vụ
web ở host/port khác, thêm origin đó vào `CORS_ALLOWED_ORIGINS` (các origin phân tách bằng dấu phẩy).
Khi deploy, đặt `APP_ENV=production`, cung cấp `JWT_SECRET` riêng dài ít nhất 32 ký tự và
`CORS_ALLOWED_ORIGINS` chỉ gồm domain tin cậy; server sẽ từ chối khởi động production nếu thiếu
cấu hình này hoặc còn dùng secret dev.

**Tài khoản mới đăng ký được tặng sẵn 400 điểm** (dev-only, xem ghi chú trong `auth.rs`) để bạn
test mua đồ/combo ngay mà không phải check-in nhiều ngày chờ đủ điểm. Xoá dòng đó khi không cần
nữa.

## Test nhanh bằng curl — đủ 3 luồng chính (không cần mở Unity)

### 1. Luồng đăng ký + check-in (quét GPS)

```bash
curl -X POST http://localhost:44321/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"123456","display_name":"Test User"}'
# → {"success":true,"token":"...", ...} — copy token dùng cho các lệnh dưới

TOKEN="dán-token-vào-đây"

# Check-in tại quán cà phê mẫu (toạ độ khớp store.rs)
curl -X POST http://localhost:44321/v1/checkin \
  -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"partner_id":"cafe-01","latitude":10.7730,"longitude":106.6985,"accuracy_meters":8,"client_timestamp_unix":0}'
```

### 2. Luồng search bản đồ (API mới — bản cũ chưa có)

```bash
# Tìm theo tên, không phân biệt hoa/thường, khớp một phần
curl "http://localhost:44321/v1/search?q=c%C3%A0%20ph%C3%AA"

# Toàn bộ điểm đối tác đang active, để vẽ pin lên bản đồ
curl http://localhost:44321/v1/partners
```

### 3. Luồng tủ đồ + combo + voucher (quét mã)

```bash
# Xem tủ đồ (có sẵn 400 điểm sau khi đăng ký)
curl http://localhost:44321/v1/wardrobe -H "Authorization: Bearer $TOKEN"

# Mua đủ 4 món (ao_den, quan_den, non, giay) — món cuối sẽ trả về "combo_unlocked"
curl -X POST http://localhost:44321/v1/shop/buy \
  -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"item_id":"ao_den"}'
# ... lặp lại với quan_den, non, giay

# Xem voucher vừa nhận
curl http://localhost:44321/v1/vouchers -H "Authorization: Bearer $TOKEN"
```

### Luồng chủ quán — tạo quán (tự gắn combo), mời nhân viên, tạo event, quét mã

Dùng chính tài khoản `$TOKEN` ở trên (nó sẽ tự chuyển thành `owner` khi tạo quán):

```bash
# 1. Tạo quán — gắn với partner_id "cafe-01" VÀ gắn luôn combo "coffee_black" (combo_id).
#    Bản in-memory không có psql nên field combo_id thay cho lệnh
#    `UPDATE combo_rules SET business_id=...` của bản cũ.
curl -X POST http://localhost:44321/v1/businesses \
  -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"name":"Cafe Test","category":"quan_nuoc","partner_id":"cafe-01","combo_id":"coffee_black"}'
# → {"success":true,"business_id":"...","message":"Tạo quán thành công"}

BIZ="dán-business_id-vào-đây"

# 2. Đăng ký thêm 1 tài khoản làm nhân viên
curl -X POST http://localhost:44321/v1/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"staff@test.com","password":"123456","display_name":"Nhan Vien"}'

curl -X POST http://localhost:44321/v1/businesses/$BIZ/staff \
  -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"email":"staff@test.com"}'
# → {"success":true,"message":"Đã thêm nhân viên"}

# 3. Tạo event — tự động gửi thông báo tới mọi khách đã check-in ở quán này
curl -X POST http://localhost:44321/v1/businesses/$BIZ/events \
  -H "Content-Type: application/json" -H "Authorization: Bearer $TOKEN" \
  -d '{"title":"Giảm 20%","body":"Áp dụng cuối tuần","starts_at":"2026-09-20T00:00:00Z","ends_at":"2026-09-22T00:00:00Z"}'

# 4. Khách hàng ($TOKEN) xem thông báo vừa nhận
curl http://localhost:44321/v1/notifications -H "Authorization: Bearer $TOKEN"

# 5. Đăng nhập bằng tài khoản staff để lấy TOKEN riêng, rồi mới quét được voucher
STAFF_TOKEN="dán-token-của-staff@test.com-vào-đây"

curl -X POST http://localhost:44321/v1/vouchers/redeem \
  -H "Content-Type: application/json" -H "Authorization: Bearer $STAFF_TOKEN" \
  -d '{"code":"dán-code-voucher-lấy-từ-/v1/vouchers"}'
```

## Ráp vào Unity

1. Copy toàn bộ `unity/Assets/Scripts/*.cs` vào `Assets/Scripts/` trong project Unity của bạn.
2. `PartnerCheckin.cs` chỉ dùng `Input.location` (GPS gốc của Unity) — không cần cài SDK bản đồ nào.
3. Tạo GameObject `SessionManager` → gắn `SessionManager.cs`.
4. Tạo GameObject `AuthManager` → gắn `AuthManager.cs`, điền `Api Base Url` (mặc định `http://localhost:44321`; khi chạy trên thiết bị thật, dùng địa chỉ LAN của máy chạy server), nối `OnAuthSuccess`/`OnAuthFailed` với UI màn đăng nhập.
5. Tạo GameObject `WardrobeManager` → gắn `WardrobeManager.cs`, đặt cùng `Api Base Url` với `AuthManager`, nối `OnWardrobeLoaded`/`OnComboUnlocked`.
6. Tạo GameObject `CheckinManager` → gắn `PartnerCheckin.cs`.
7. Cho luồng search map: gọi `GET /v1/search?q=...` và `GET /v1/partners` (chưa có script Unity
   sẵn cho phần này — nếu cần mình viết thêm `MapSearchManager.cs` theo đúng style 2 script kia).

## Bản đồ: OpenFreeMap

Vẫn dùng [OpenFreeMap](https://openfreemap.org/) — miễn phí, không cần API key, dữ liệu từ
OpenStreetMap. Nó chỉ phục vụ vector tile (không có search/geocode/ảnh tĩnh), nên phần search
tên quán được tự làm trên chính dữ liệu `partner_locations`/`businesses` trong server
(`GET /v1/search`) thay vì gọi OpenFreeMap.

Hai dashboard, mở thẳng bằng trình duyệt là chạy, không cần build:

| File | Bản đồ | Khi nào dùng |
|---|---|---|
| `web/owner-dashboard-3d.html` | CesiumJS + OpenFreeMap (3D) | Mặc định — địa cầu 3D |
| `web/owner-dashboard.html` | MapLibre + OpenFreeMap (2D) | Dự phòng, ổn định hơn |

## API đầy đủ

| Method | Endpoint | Ai gọi được | Ghi chú |
|---|---|---|---|
| POST | `/v1/auth/register` | Ai cũng gọi được | Tặng sẵn 400 điểm (dev-only) |
| POST | `/v1/auth/login` | Ai cũng gọi được | |
| POST | `/v1/checkin` | User đã đăng nhập | GPS check-in, 1 lần/quán/ngày |
| GET | `/v1/wardrobe` | User đã đăng nhập | |
| POST | `/v1/shop/buy` | User đã đăng nhập | Trừ điểm, có thể trả `combo_unlocked` |
| POST | `/v1/shop/equip` | User đã đăng nhập | Không tốn điểm |
| GET | `/v1/vouchers` | User đã đăng nhập | |
| POST | `/v1/vouchers/redeem` | Staff/owner của đúng quán phát hành | |
| GET | `/v1/partners` | Ai cũng gọi được | Danh sách điểm GPS để vẽ pin |
| GET | `/v1/search?q=` | Ai cũng gọi được | Tìm quán theo tên (luồng bản đồ) |
| POST | `/v1/businesses` | User đã đăng nhập (tự thành `owner`) | Có thể kèm `combo_id` để tự gắn combo |
| GET | `/v1/businesses/mine` | User đã đăng nhập | |
| POST | `/v1/businesses/:id/staff` | Chỉ **chủ** quán đó | |
| DELETE | `/v1/businesses/:id/staff/:user_id` | Chỉ **chủ** quán đó | |
| POST | `/v1/businesses/:id/events` | Chủ **hoặc** nhân viên quán đó | Tự gửi thông báo |
| GET | `/v1/notifications` | Khách hàng (chỉ thấy của mình) | |
| POST | `/v1/notifications/:id/read` | Khách hàng (chỉ của mình) | |

## Còn thiếu — cần làm trước khi launch thật

- **Rate limiting**, **CORS** đang mở `Any` (chỉ dùng dev).
- **Chống GPS spoofing** — cần thêm lớp xác nhận (nhân viên duyệt, hoặc quét QR tại quầy) nếu quà có giá trị thật.
- **Refresh token** — JWT hết hạn sau 30 ngày, chưa có cơ chế làm mới không cần đăng nhập lại.
- **Thanh toán thật** (MoMo/ZaloPay/VietQR webhook) để cộng điểm khi nạp tiền.
- **Dữ liệu bền vững** — xem mục dưới.

## Chuyển sang Postgres (khi sẵn sàng)

`migrations/schema.sql` đã có sẵn, cấu trúc bảng khớp 1-1 với các struct trong `server/src/store.rs`
(vd `users` ↔ `User`, `vouchers` ↔ `Voucher`...). Khi cần dữ liệu bền vững thật:

1. `psql -d cityquest -f migrations/schema.sql`
2. Thêm lại `sqlx` vào `Cargo.toml` (đã có sẵn cấu trúc bảng phù hợp).
3. Đổi `State<Arc<Store>>` trong từng handler thành `State<PgPool>`, thay các thao tác
   `data.xxx.get/insert/...` bằng `sqlx::query!`. Logic nghiệp vụ (ai được làm gì, khi nào phát
   voucher, cách tính khoảng cách GPS...) giữ nguyên — chỉ đổi lớp lưu trữ.
4. Xoá dòng tặng 400 điểm dev-only trong `auth.rs`.

Nhắn cho mình khi tới bước này, mình sẽ viết lại phần sqlx cho khớp.
