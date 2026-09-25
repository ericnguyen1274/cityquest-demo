-- schema.sql
-- Chạy: psql -d your_db -f schema.sql

CREATE EXTENSION IF NOT EXISTS "pgcrypto"; -- cho gen_random_uuid()

CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email           TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    points          INT NOT NULL DEFAULT 0,
    role            TEXT NOT NULL DEFAULT 'customer' CHECK (role IN ('customer', 'staff', 'owner')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- An toàn cho DB đã tồn tại từ trước (CREATE TABLE IF NOT EXISTS ở trên sẽ không tự thêm cột mới).
ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'customer';
DO $$ BEGIN
    ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('customer', 'staff', 'owner'));
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

CREATE TABLE IF NOT EXISTS partner_locations (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    latitude        DOUBLE PRECISION NOT NULL,
    longitude       DOUBLE PRECISION NOT NULL,
    radius_meters   REAL NOT NULL DEFAULT 25,
    points_reward   INT NOT NULL DEFAULT 20,
    active          BOOLEAN NOT NULL DEFAULT true
);

CREATE TABLE IF NOT EXISTS checkins (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id),
    partner_id      TEXT NOT NULL REFERENCES partner_locations(id),
    latitude        DOUBLE PRECISION NOT NULL,
    longitude       DOUBLE PRECISION NOT NULL,
    accuracy_meters REAL,
    checked_in_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Cột ngày tường minh, do ứng dụng set khi insert (thay vì ép kiểu checked_in_at::date
    -- trong index — Postgres không cho phép vì phép ép đó phụ thuộc timezone của session
    -- nên không phải hàm IMMUTABLE, sẽ báo lỗi khi tạo index).
    checkin_date    DATE NOT NULL DEFAULT CURRENT_DATE
);

ALTER TABLE checkins ADD COLUMN IF NOT EXISTS checkin_date DATE NOT NULL DEFAULT CURRENT_DATE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_one_checkin_per_partner_per_day
    ON checkins (user_id, partner_id, checkin_date);

-- ===== Tủ đồ / Combo / Voucher =====

CREATE TABLE IF NOT EXISTS items (
    id      TEXT PRIMARY KEY,       -- 'ao_do', 'ao_den', 'quan_do', 'quan_den', 'non', 'giay'
    name    TEXT NOT NULL,
    slot    TEXT NOT NULL,          -- 'ao' | 'quan' | 'non' | 'giay' — 1 slot chỉ mặc được 1 item cùng lúc
    cost    INT NOT NULL
);

CREATE TABLE IF NOT EXISTS user_items (
    user_id     UUID NOT NULL REFERENCES users(id),
    item_id     TEXT NOT NULL REFERENCES items(id),
    equipped    BOOLEAN NOT NULL DEFAULT true,
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, item_id)
);

CREATE TABLE IF NOT EXISTS combo_rules (
    id                      TEXT PRIMARY KEY,
    required_items          TEXT[] NOT NULL,   -- danh sách item_id bắt buộc phải đang equip cùng lúc
    voucher_title           TEXT NOT NULL,
    voucher_validity_days   INT NOT NULL DEFAULT 30
);

CREATE TABLE IF NOT EXISTS vouchers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id),
    combo_id    TEXT NOT NULL REFERENCES combo_rules(id),
    code        TEXT NOT NULL,
    title       TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    used        BOOLEAN NOT NULL DEFAULT false,
    redeemed_by UUID REFERENCES users(id),   -- nhân viên/chủ quán đã quét mã này
    redeemed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE vouchers ADD COLUMN IF NOT EXISTS redeemed_by UUID REFERENCES users(id);
ALTER TABLE vouchers ADD COLUMN IF NOT EXISTS redeemed_at TIMESTAMPTZ;

-- Mỗi user chỉ nhận 1 voucher / combo (tránh spam equip/unequip lấy voucher liên tục)
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_voucher_per_combo
    ON vouchers (user_id, combo_id);

-- ===== Cơ sở kinh doanh / Nhân viên =====

CREATE TABLE IF NOT EXISTS businesses (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    UUID NOT NULL REFERENCES users(id),
    name        TEXT NOT NULL,
    -- 'quan_an' | 'quan_nuoc' | 'tiem_mo_hinh' | 'khac'
    category    TEXT NOT NULL DEFAULT 'khac',
    partner_id  TEXT REFERENCES partner_locations(id), -- vị trí GPS để khách check-in
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_businesses_owner ON businesses (owner_id);

-- Nhân viên được chủ quán cấp quyền quét voucher. Tách bảng riêng (thay vì chỉ
-- dựa vào users.role) vì 1 người có thể làm ở nhiều quán, và quyền phải gắn với
-- từng quán cụ thể — nếu không thì staff quán A sẽ quét được voucher của quán B.
CREATE TABLE IF NOT EXISTS business_staff (
    business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users(id),
    added_by    UUID NOT NULL REFERENCES users(id),
    added_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (business_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_business_staff_user ON business_staff (user_id);

-- ===== Sự kiện / Thông báo =====

CREATE TABLE IF NOT EXISTS events (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    business_id UUID NOT NULL REFERENCES businesses(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    starts_at   TIMESTAMPTZ NOT NULL,
    ends_at     TIMESTAMPTZ NOT NULL,
    created_by  UUID NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_events_business ON events (business_id);

CREATE TABLE IF NOT EXISTS notifications (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id),
    event_id    UUID REFERENCES events(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL,
    read        BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index cho truy vấn "thông báo chưa đọc của tôi, mới nhất trước"
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread
    ON notifications (user_id, read, created_at DESC);

-- Không gửi trùng 1 event cho cùng 1 người 2 lần
CREATE UNIQUE INDEX IF NOT EXISTS idx_one_notification_per_event_per_user
    ON notifications (user_id, event_id);

-- ===== Gắn voucher với quán phát hành =====
-- Trước đây combo_rules là toàn hệ thống, nên staff bất kỳ quán nào cũng redeem
-- được voucher không phải của quán mình. Giờ mỗi combo thuộc về 1 quán.
ALTER TABLE combo_rules ADD COLUMN IF NOT EXISTS business_id UUID REFERENCES businesses(id);
ALTER TABLE vouchers    ADD COLUMN IF NOT EXISTS business_id UUID REFERENCES businesses(id);

-- ===== Dữ liệu mẫu =====

INSERT INTO partner_locations (id, name, latitude, longitude, radius_meters, points_reward)
VALUES ('cafe-01', 'Quán cà phê ABC', 10.7730, 106.6985, 25, 20)
ON CONFLICT (id) DO NOTHING;

INSERT INTO items (id, name, slot, cost) VALUES
    ('ao_do',   'Áo đỏ',        'ao',   60),
    ('ao_den',  'Áo đen',       'ao',   80),
    ('quan_do', 'Quần đỏ',      'quan', 60),
    ('quan_den','Quần đen',     'quan', 80),
    ('non',     'Nón',          'non',  70),
    ('giay',    'Giày',         'giay', 90)
ON CONFLICT (id) DO NOTHING;

INSERT INTO combo_rules (id, required_items, voucher_title, voucher_validity_days) VALUES
    ('coffee_black', ARRAY['ao_den','quan_den','non','giay'], 'Ly cà phê miễn phí', 30)
ON CONFLICT (id) DO NOTHING;
