// WardrobeManager.cs
//
// Gọi GET /v1/wardrobe, POST /v1/shop/buy, POST /v1/shop/equip, GET /v1/vouchers.
// Toàn bộ logic combo/voucher nằm ở Server Rust (shop.rs) — script này chỉ hiển thị
// kết quả server trả về, không tự quyết định gì cả (tránh gian lận phía client).

using System;
using System.Collections;
using System.Collections.Generic;
using System.Text;
using UnityEngine;
using UnityEngine.Events;
using UnityEngine.Networking;

namespace CityQuest.Gameplay
{
    [Serializable] public class WardrobeLoadedEvent : UnityEvent<WardrobeState> { }
    [Serializable] public class ComboUnlockedEvent : UnityEvent<string, string> { } // (voucher_code, voucher_title)
    [Serializable] public class ShopErrorEvent : UnityEvent<string> { }

    [Serializable]
    public class ItemStatus
    {
        public string id;
        public string name;
        public string slot;
        public int cost;
        public bool owned;
        public bool equipped;
    }

    [Serializable]
    public class WardrobeState
    {
        public int points;
        public List<ItemStatus> items;
    }

    public class WardrobeManager : MonoBehaviour
    {
        [SerializeField] private string apiBaseUrl = "https://api.cityquest.app";

        public WardrobeLoadedEvent OnWardrobeLoaded;
        public ComboUnlockedEvent OnComboUnlocked;
        public ShopErrorEvent OnError;

        public void RefreshWardrobe()
        {
            StartCoroutine(GetWardrobeCoroutine());
        }

        public void BuyItem(string itemId)
        {
            StartCoroutine(PostShopAction("/v1/shop/buy", "{\"item_id\":\"" + itemId + "\"}"));
        }

        public void EquipItem(string itemId)
        {
            StartCoroutine(PostShopAction("/v1/shop/equip", "{\"item_id\":\"" + itemId + "\"}"));
        }

        private IEnumerator GetWardrobeCoroutine()
        {
            using (var req = UnityWebRequest.Get(apiBaseUrl + "/v1/wardrobe"))
            {
                AttachAuth(req);
                yield return req.SendWebRequest();

                if (req.result == UnityWebRequest.Result.Success)
                {
                    var state = JsonUtility.FromJson<WardrobeState>(req.downloadHandler.text);
                    OnWardrobeLoaded?.Invoke(state);
                }
                else
                {
                    OnError?.Invoke("Không tải được tủ đồ: " + req.error);
                }
            }
        }

        private IEnumerator PostShopAction(string endpoint, string json)
        {
            using (var req = new UnityWebRequest(apiBaseUrl + endpoint, "POST"))
            {
                byte[] bodyRaw = Encoding.UTF8.GetBytes(json);
                req.uploadHandler = new UploadHandlerRaw(bodyRaw);
                req.downloadHandler = new DownloadHandlerBuffer();
                req.SetRequestHeader("Content-Type", "application/json");
                AttachAuth(req);

                yield return req.SendWebRequest();

                if (req.result == UnityWebRequest.Result.Success)
                {
                    var res = JsonUtility.FromJson<ShopActionResponse>(req.downloadHandler.text);
                    if (res != null && res.success)
                    {
                        if (res.combo_unlocked != null && !string.IsNullOrEmpty(res.combo_unlocked.voucher_code))
                        {
                            OnComboUnlocked?.Invoke(res.combo_unlocked.voucher_code, res.combo_unlocked.voucher_title);
                        }
                        RefreshWardrobe(); // đồng bộ lại toàn bộ trạng thái sau khi mua/mặc
                    }
                    else
                    {
                        OnError?.Invoke(res != null ? res.message : "Thao tác thất bại");
                    }
                }
                else
                {
                    OnError?.Invoke("Lỗi kết nối: " + req.error);
                }
            }
        }

        private void AttachAuth(UnityWebRequest req)
        {
            string token = SessionManager.Instance != null ? SessionManager.Instance.AuthToken : null;
            if (!string.IsNullOrEmpty(token))
            {
                req.SetRequestHeader("Authorization", "Bearer " + token);
            }
        }

        [Serializable]
        private class ShopActionResponse
        {
            public bool success;
            public int points;
            public ComboUnlockedBody combo_unlocked;
            public string message;
        }

        [Serializable]
        private class ComboUnlockedBody
        {
            public string voucher_code;
            public string voucher_title;
        }
    }
}
