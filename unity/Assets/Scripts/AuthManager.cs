// AuthManager.cs
//
// Gọi POST /v1/auth/register và /v1/auth/login, lưu JWT trả về vào SessionManager
// để các script khác (PartnerCheckin, WardrobeManager) tự động gắn vào header.

using System;
using System.Collections;
using System.Text;
using UnityEngine;
using UnityEngine.Events;
using UnityEngine.Networking;

namespace CityQuest.Gameplay
{
    [Serializable] public class AuthSuccessEvent : UnityEvent { }
    [Serializable] public class AuthFailedEvent : UnityEvent<string> { }

    public class AuthManager : MonoBehaviour
    {
        [SerializeField] private string apiBaseUrl = "https://api.cityquest.app";

        public AuthSuccessEvent OnAuthSuccess;
        public AuthFailedEvent OnAuthFailed;

        public void Register(string email, string password, string displayName)
        {
            var body = new RegisterBody { email = email, password = password, display_name = displayName };
            StartCoroutine(PostAuth("/v1/auth/register", JsonUtility.ToJson(body)));
        }

        public void Login(string email, string password)
        {
            var body = new LoginBody { email = email, password = password };
            StartCoroutine(PostAuth("/v1/auth/login", JsonUtility.ToJson(body)));
        }

        private IEnumerator PostAuth(string endpoint, string json)
        {
            using (var req = new UnityWebRequest(apiBaseUrl + endpoint, "POST"))
            {
                byte[] bodyRaw = Encoding.UTF8.GetBytes(json);
                req.uploadHandler = new UploadHandlerRaw(bodyRaw);
                req.downloadHandler = new DownloadHandlerBuffer();
                req.SetRequestHeader("Content-Type", "application/json");

                yield return req.SendWebRequest();

                if (req.result == UnityWebRequest.Result.Success)
                {
                    var res = JsonUtility.FromJson<AuthResponseBody>(req.downloadHandler.text);
                    if (res != null && res.success && !string.IsNullOrEmpty(res.token))
                    {
                        SessionManager.Instance.AuthToken = res.token;
                        OnAuthSuccess?.Invoke();
                    }
                    else
                    {
                        OnAuthFailed?.Invoke(res != null ? res.message : "Đăng nhập/đăng ký thất bại");
                    }
                }
                else
                {
                    OnAuthFailed?.Invoke("Lỗi kết nối: " + req.error);
                }
            }
        }

        [Serializable] private class RegisterBody { public string email; public string password; public string display_name; }
        [Serializable] private class LoginBody { public string email; public string password; }
        [Serializable] private class AuthResponseBody { public bool success; public string token; public string message; }
    }
}
