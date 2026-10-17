// Quicktest.cs
//
// Gắn vào 1 GameObject trống bất kỳ trong scene, cùng với AuthManager và WardrobeManager
// (kéo-thả 2 component đó vào field bên dưới). Bấm Play — nó tự chạy Đăng ký → Đăng nhập
// → Lấy tủ đồ và in hết ra Console, giúp test nhanh toàn bộ pipeline mà chưa cần dựng UI.
//
// Sau khi test xong, XÓA script này khỏi scene (nó chỉ để test, không phải code sản phẩm thật).

using UnityEngine;

namespace CityQuest.Gameplay
{
    public class Quicktest : MonoBehaviour
    {
        [SerializeField] private AuthManager authManager;
        [SerializeField] private WardrobeManager wardrobeManager;

        [Header("Tài khoản test — mỗi lần Play nên đổi email vì server chặn trùng email")]
        [SerializeField] private string testEmail = "editor-test@example.com";
        [SerializeField] private string testPassword = "123456";
        [SerializeField] private string testDisplayName = "Editor Tester";

        private void Start()
        {
            if (authManager == null || wardrobeManager == null)
            {
                Debug.LogError("[Quicktest] Chưa kéo AuthManager / WardrobeManager vào Inspector.");
                return;
            }

            authManager.OnAuthSuccess.AddListener(HandleAuthSuccess);
            authManager.OnAuthFailed.AddListener(msg => Debug.LogWarning("[Quicktest] Auth thất bại: " + msg));

            wardrobeManager.OnWardrobeLoaded.AddListener(state =>
            {
                Debug.Log($"[Quicktest] Tủ đồ tải xong. Điểm hiện có: {state.points}");
                foreach (var item in state.items)
                {
                    Debug.Log($"  - {item.name} ({item.slot}, {item.cost}đ) | sở hữu: {item.owned} | đang mặc: {item.equipped}");
                }
            });
            wardrobeManager.OnComboUnlocked.AddListener((code, title) =>
                Debug.Log($"[Quicktest] MỞ KHÓA COMBO! Voucher: {title} — mã {code}"));
            wardrobeManager.OnError.AddListener(msg => Debug.LogWarning("[Quicktest] Lỗi shop: " + msg));

            Debug.Log("[Quicktest] Đang thử đăng ký tài khoản test...");
            authManager.Register(testEmail, testPassword, testDisplayName);
        }

        private void HandleAuthSuccess()
        {
            Debug.Log("[Quicktest] Đăng ký/Đăng nhập thành công, token đã lưu vào SessionManager.");
            Debug.Log("[Quicktest] Đang lấy dữ liệu tủ đồ...");
            wardrobeManager.RefreshWardrobe();
        }
    }
}
