using System;
using System.Collections;
using System.Text;
using UnityEngine;
using UnityEngine.Networking;
using UnityEngine.UI;

/// <summary>
/// Check-in đối tác bằng GPS thiết bị.
///
/// KHÔNG dùng map provider nào (trước đây file này gọi TomTom Static Image API).
/// Lý do: việc check-in chỉ cần (a) toạ độ GPS của người chơi và (b) toạ độ quán,
/// rồi tính khoảng cách bằng Haversine. Cả hai đều không cần tới dịch vụ bản đồ —
/// GPS lấy từ Unity Input.location, còn toạ độ quán lấy từ server của mình.
/// Bản đồ chỉ là thứ để NHÌN, và với game phong cách Animal Crossing/Play Together
/// thì thế giới trong game là 3D stylized chứ không phải bản đồ đường phố thật.
///
/// Nếu vẫn muốn hiển thị bản đồ thật, xem thư mục web/ — có 2 dashboard dùng
/// OpenFreeMap (miễn phí, không cần API key): bản 3D (CesiumJS) và bản 2D (MapLibre).
/// Cả hai đều là web, KHÔNG chạy native trong Unity. OpenFreeMap chỉ phục vụ vector
/// tiles, không có API trả ảnh PNG tĩnh nên không gán thẳng vào RawImage được.
/// Muốn đưa vào Unity thì qua WebView, hoặc tự dựng server render tile thành raster.
/// (Cesium có plugin "Cesium for Unity", nhưng plugin đó chưa hỗ trợ vector tiles MVT —
/// đây vẫn đang là yêu cầu tính năng trên diễn đàn Cesium, chưa phải thứ dùng được.)
///
/// LƯU Ý BẢO MẬT: toạ độ do client gửi lên luôn có thể bị giả mạo (GPS spoofing).
/// Server đã kiểm tra lại khoảng cách và giới hạn 1 check-in/quán/ngày, nhưng nếu
/// làm thật với tiền/quà có giá trị thì cần thêm biện pháp chống gian lận
/// (ví dụ: nhân viên quán xác nhận, hoặc quét QR tại quầy).
/// </summary>
public class PartnerCheckin : MonoBehaviour
{
    [Header("Server")]
    [SerializeField] private string serverUrl = "http://localhost:44321";

    [Header("Đối tác")]
    [SerializeField] private string partnerId = "cafe-01";

    [Header("UI References")]
    [SerializeField] private Text statusText;
    [SerializeField] private Button checkinButton;

    private double userLat;
    private double userLon;
    private float accuracyMeters;
    private bool locationReady = false;

    [Serializable]
    private class CheckinRequest
    {
        public string partner_id;
        public double latitude;
        public double longitude;
        public float accuracy_meters;
        public long client_timestamp_unix;
    }

    [Serializable]
    private class CheckinResponse
    {
        public bool success;
        public int points_awarded;
        public string message;
    }

    private void Start()
    {
        if (checkinButton != null)
        {
            checkinButton.interactable = false;
            checkinButton.onClick.AddListener(() => StartCoroutine(DoCheckin()));
        }
        StartCoroutine(InitLocation());
    }

    // ---------- 1. Lấy GPS thật của thiết bị ----------
    private IEnumerator InitLocation()
    {
        if (!Input.location.isEnabledByUser)
        {
            SetStatus("GPS chưa được bật trên thiết bị.");
            yield break;
        }

        Input.location.Start(10f, 10f);

        int maxWait = 15;
        while (Input.location.status == LocationServiceStatus.Initializing && maxWait > 0)
        {
            yield return new WaitForSeconds(1);
            maxWait--;
        }

        if (Input.location.status != LocationServiceStatus.Running)
        {
            SetStatus("Không lấy được vị trí GPS.");
            Input.location.Stop();
            yield break;
        }

        userLat = Input.location.lastData.latitude;
        userLon = Input.location.lastData.longitude;
        accuracyMeters = Input.location.lastData.horizontalAccuracy;
        locationReady = true;

        Input.location.Stop();

        SetStatus($"Đã xác định vị trí (±{accuracyMeters:F0}m). Bấm check-in.");
        if (checkinButton != null) checkinButton.interactable = true;
    }

    // ---------- 2. Gửi lên server — server mới là bên quyết định hợp lệ hay không ----------
    private IEnumerator DoCheckin()
    {
        if (!locationReady)
        {
            SetStatus("Chưa có vị trí GPS.");
            yield break;
        }

        string token = CityQuest.Gameplay.SessionManager.Instance != null
            ? CityQuest.Gameplay.SessionManager.Instance.AuthToken
            : null;
        if (string.IsNullOrEmpty(token))
        {
            SetStatus("Bạn cần đăng nhập trước.");
            yield break;
        }

        if (checkinButton != null) checkinButton.interactable = false;

        var payload = new CheckinRequest
        {
            partner_id = partnerId,
            latitude = userLat,
            longitude = userLon,
            accuracy_meters = accuracyMeters,
            client_timestamp_unix = DateTimeOffset.UtcNow.ToUnixTimeSeconds(),
        };

        string json = JsonUtility.ToJson(payload);

        using (UnityWebRequest req = new UnityWebRequest($"{serverUrl}/v1/checkin", "POST"))
        {
            req.uploadHandler = new UploadHandlerRaw(Encoding.UTF8.GetBytes(json));
            req.downloadHandler = new DownloadHandlerBuffer();
            req.SetRequestHeader("Content-Type", "application/json");
            req.SetRequestHeader("Authorization", $"Bearer {token}");

            yield return req.SendWebRequest();

            if (req.result != UnityWebRequest.Result.Success && req.responseCode == 0)
            {
                SetStatus($"Lỗi kết nối: {req.error}");
            }
            else
            {
                var res = JsonUtility.FromJson<CheckinResponse>(req.downloadHandler.text);
                if (res != null && res.success)
                    SetStatus($"✅ Check-in thành công! +{res.points_awarded} điểm");
                else
                    SetStatus($"❌ {res?.message ?? "Check-in thất bại"}");
            }
        }

        if (checkinButton != null) checkinButton.interactable = true;
    }

    private void SetStatus(string m)
    {
        Debug.Log($"[PartnerCheckin] {m}");
        if (statusText != null) statusText.text = m;
    }
}
