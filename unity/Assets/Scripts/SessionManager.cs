// SessionManager.cs
// Tách riêng từ PartnerCheckin.cs để dễ kéo-thả trong Unity Editor.

using UnityEngine;

namespace CityQuest.Gameplay
{
    /// <summary>
    /// Placeholder tối giản cho nơi lưu JWT sau khi đăng nhập.
    /// Thay bằng hệ thống quản lý phiên đăng nhập thật của bạn (ví dụ lưu trong bộ nhớ,
    /// không lưu plaintext vào PlayerPrefs).
    /// </summary>
    public class SessionManager : MonoBehaviour
    {
        public static SessionManager Instance { get; private set; }
        public string AuthToken { get; set; }

        private void Awake()
        {
            if (Instance != null && Instance != this) { Destroy(gameObject); return; }
            Instance = this;
            DontDestroyOnLoad(gameObject);
        }
    }
}
