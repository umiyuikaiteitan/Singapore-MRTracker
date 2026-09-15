#include <Arduino.h>
#include <HTTPClient.h>
#include <WiFi.h>
#include <time.h>
#include <freertos/FreeRTOS.h>
#include <freertos/queue.h>
#include <freertos/task.h>

// PlatformIO checks this exact project file before preprocessing. An include
// probe alone can accidentally discover one of the ESP32 SDK's config.h files.
#if defined(MRT_PRIVATE_CONFIG)
#include "../include/config.h"
#else
// Example settings intentionally connect to no real credentials. This also
// permits reproducible compilation before a private config.h is installed.
#include "config.example.h"
#endif
#include "frame_decoder.h"

#if ENABLE_LEDS
#include <Adafruit_NeoPixel.h>
#endif
#if ENABLE_EPAPER
#include <GxEPD2_BW.h>
#include <SPI.h>
#endif

static_assert(LED_COUNT > 0 && LED_COUNT <= mrt::kMaximumPixels, "LED_COUNT must be 1..512");
static_assert(LED_BRIGHTNESS >= 0 && LED_BRIGHTNESS <= 255, "Brightness must be 0..255");
static_assert(LED_CURRENT_LIMIT_MA >= LED_COUNT, "Budget must cover pixel idle current");
static_assert(FRAME_POLL_MS >= 1000 && FRAME_POLL_MS < 0x80000000UL, "Invalid poll interval");
static_assert(EPAPER_REFRESH_MS >= 60000 && EPAPER_REFRESH_MS < 0x80000000UL,
              "Full e-paper refresh must be at least 60 seconds apart");

namespace {
constexpr size_t kMaxBodyBytes = mrt::kMaxBodyBytes;
constexpr uint32_t kNetworkTimeoutMs = 4000;
constexpr uint32_t kReconnectMs = 10000;
using Source = mrt::Source;
using Row = mrt::Row;
using Frame = mrt::Frame<LED_COUNT>;

Frame current;
Frame incoming;
mrt::Lease lease;
char body[kMaxBodyBytes + 1];
const char* status = "WAITING FOR DATA";
bool have_frame = false;
bool screen_drawn = false;
uint32_t last_screen = 0;
uint32_t last_poll = 0;
uint32_t last_reconnect = 0;
bool attempted_poll = false;

struct LedMessage {
  mrt::Pixel pixels[LED_COUNT] = {};
  size_t count = 0;
  uint32_t started = 0;
  uint32_t duration = 0;
};
#if ENABLE_LEDS
Adafruit_NeoPixel leds(LED_COUNT, LED_PIN, NEO_GRB + NEO_KHZ800);
QueueHandle_t led_queue = nullptr;

// Only this task writes the pixel buffer after setup. Network requests and
// blocking e-paper refreshes cannot postpone LED expiry.
void ledTask(void*) {
  LedMessage active;
  bool lit = false;
  for (;;) {
    if (xQueueReceive(led_queue, &active, pdMS_TO_TICKS(25)) == pdTRUE) {
      leds.clear();
      lit = active.duration && !mrt::elapsed(millis(), active.started, active.duration);
      if (lit) {
        for (size_t i = 0; i < active.count; ++i) {
          const auto& p = active.pixels[i];
          leds.setPixelColor(p.index, p.r, p.g, p.b);
        }
      }
      leds.show();
    }
    if (lit && mrt::elapsed(millis(), active.started, active.duration)) {
      leds.clear();
      leds.show();
      lit = false;
    }
  }
}
#endif

void clearLeds() {
#if ENABLE_LEDS
  LedMessage blank;
  if (led_queue) xQueueOverwrite(led_queue, &blank);
#endif
}

void submitLeds(const Frame& frame) {
#if ENABLE_LEDS
  LedMessage message;
  if (frame.source == Source::Realtime || frame.source == Source::Schedule) {
    message.count = frame.pixel_count;
    memcpy(message.pixels, frame.pixels, sizeof(message.pixels));
    mrt::limitCurrent(message.pixels, message.count, LED_COUNT,
                      LED_BRIGHTNESS, LED_CURRENT_LIMIT_MA);
    message.started = lease.started();
    message.duration = lease.duration();
  }
  if (led_queue) xQueueOverwrite(led_queue, &message);
#endif
}

void fail(const char* reason) {
  status = reason;
  lease.invalidate();
  clearLeds();
  Serial.println(reason); // Never print credentials, endpoint, or response body.
}

// HTTP/1.0 requests a bounded Content-Length response from the local service.
// Chunked, redirected, compressed, or unbounded responses fail closed.
bool fetchBody(size_t& received) {
  if (strncmp(FRAME_URL, "http://", 7) != 0) { fail("CONFIGURE TRUSTED LAN HTTP URL"); return false; }
  WiFiClient client;
  HTTPClient http;
  http.useHTTP10(true);
  http.setConnectTimeout(kNetworkTimeoutMs);
  http.setTimeout(kNetworkTimeoutMs);
  http.setFollowRedirects(HTTPC_DISABLE_FOLLOW_REDIRECTS);
  const char* keys[] = {"Content-Encoding", "Transfer-Encoding"};
  http.collectHeaders(keys, 2);
  if (!http.begin(client, FRAME_URL)) { fail("HTTP SETUP FAILED"); return false; }
  http.addHeader("Accept", "application/json");
  http.addHeader("Accept-Encoding", "identity");
  const int code = http.GET();
  const int length = http.getSize();
  if (code != HTTP_CODE_OK || length <= 0 || size_t(length) > kMaxBodyBytes ||
      http.header("Content-Encoding").length() != 0 ||
      http.header("Transfer-Encoding").length() != 0) {
    http.end();
    fail("HTTP OFFLINE OR INVALID RESPONSE");
    return false;
  }
  received = 0;
  const uint32_t started = millis();
  WiFiClient* stream = http.getStreamPtr();
  while (received < size_t(length) && !mrt::elapsed(millis(), started, kNetworkTimeoutMs)) {
    const int available = stream->available();
    if (available > 0) {
      const size_t remaining = size_t(length) - received;
      const size_t take = size_t(available) < remaining ? size_t(available) : remaining;
      const int got = stream->read(reinterpret_cast<uint8_t*>(body + received), take);
      if (got <= 0) break;
      received += size_t(got);
    } else if (!stream->connected()) break;
    else delay(2);
  }
  http.end();
  if (received != size_t(length)) { fail("INCOMPLETE HTTP FRAME"); return false; }
  body[received] = '\0';
  return true;
}

void pollFrame() {
  if (WiFi.status() != WL_CONNECTED) { fail("OFFLINE: WIFI DISCONNECTED"); return; }
  if (time(nullptr) < time_t(mrt::kEarliestClock)) { fail("WAITING FOR NTP CLOCK"); return; }
  size_t received = 0;
  if (!fetchBody(received)) return;
  const char* error = mrt::parseFrame(body, received, incoming, EXPECTED_LAYOUT_ID);
  if (error) { fail(error); return; }
  const auto accepted = lease.accept(incoming.stamp, uint64_t(time(nullptr)), millis());
  if (accepted == mrt::Acceptance::Rejected) { fail("EXPIRED OR REPLAYED FRAME"); return; }
  if (accepted == mrt::Acceptance::Duplicate) return;
  current = incoming;
  have_frame = true;
  status = nullptr;
  submitLeds(current);
}

#if ENABLE_EPAPER
GxEPD2_BW<GxEPD2_750_T7, 80> display(GxEPD2_750_T7(EPAPER_CS, EPAPER_DC, EPAPER_RST, EPAPER_BUSY));

const char* textAt(int16_t x, int16_t y, uint8_t size, const char* text, size_t max_chars) {
  display.setCursor(x, y);
  display.setTextSize(size);
  size_t count = 0;
  const unsigned char* p = reinterpret_cast<const unsigned char*>(text);
  while (*p && count < max_chars) {
    // Built-in font is ASCII. One placeholder per UTF-8 code point.
    const unsigned char character = *p++;
    if ((character & 0xc0) == 0x80) continue;
    display.write(character < 0x80 ? character : '?');
    ++count;
  }
  while ((*p & 0xc0) == 0x80) ++p;
  return reinterpret_cast<const char*>(p);
}

void formatTime(uint64_t epoch, char* target, size_t size) {
  time_t stamp = time_t(epoch + 8 * 3600); // Singapore has no daylight saving.
  struct tm singapore;
  gmtime_r(&stamp, &singapore);
  strftime(target, size, "%Y-%m-%d %H:%M:%S SGT", &singapore);
}

void renderScreen() {
  const uint32_t now = millis();
  if (screen_drawn && !mrt::elapsed(now, last_screen, EPAPER_REFRESH_MS)) return;
  if (!attempted_poll) return;
  const bool usable = have_frame && lease.alive(now) && status == nullptr &&
    current.source != Source::Unavailable;
  const char* headline = status;
  if (!headline) {
    switch (current.source) {
      case Source::Realtime: headline = "REALTIME SNAPSHOT"; break;
      case Source::Schedule: headline = "SCHEDULE ESTIMATE"; break;
      case Source::Stale: headline = "STALE LIVE INPUT - SCHEDULE SNAPSHOT"; break;
      case Source::Unavailable: headline = "SOURCE UNAVAILABLE"; break;
    }
  }
  char generated[32] = "none";
  char expiry[32] = "none";
  if (have_frame) {
    formatTime(current.stamp.generated_at, generated, sizeof(generated));
    formatTime(current.stamp.valid_until, expiry, sizeof(expiry));
  }
  display.setFullWindow();
  display.firstPage();
  do {
    display.fillScreen(GxEPD_WHITE);
    display.setTextColor(GxEPD_BLACK);
    display.setTextWrap(false);
    textAt(20, 16, 3, "SINGAPORE MRT", 42);
    textAt(20, 51, 2, have_frame ? current.station_name : EXPECTED_LAYOUT_ID, 63);
    display.drawFastHLine(20, 76, 760, GxEPD_BLACK);
    textAt(20, 89, 2, headline, 63);
    textAt(20, 119, 1, "LINE    DESTINATION                                      MIN AT SNAPSHOT / SOURCE", 125);
    if (usable) {
      for (size_t i = 0; i < current.row_count; ++i) {
        const Row& row = current.rows[i];
        const int16_t y = 141 + i * 24;
        char minutes[24];
        snprintf(minutes, sizeof(minutes), "%s%lu %s", row.approximate ? "~" : "",
                 static_cast<unsigned long>(row.minutes), row.realtime ? "RT" : "SCH");
        // Short codes remain large; longer route labels fit the same column.
        textAt(20, y, strlen(row.line) > 6 ? 1 : 2, row.line, 12);
        textAt(107, y, 2, row.destination, 37);
        textAt(570, y, 2, minutes, 17);
      }
      if (current.row_count == 0) textAt(20, 153, 2, "No departures in this snapshot.", 60);
    } else textAt(20, 153, 2, "No current departures in this snapshot.", 60);
    if (have_frame) {
      const char* continuation = textAt(20, 338, 1, current.notice, 125);
      if (*continuation) textAt(20, 350, 1, continuation, 115);
    }
    display.drawFastHLine(20, 372, 760, GxEPD_BLACK);
    char line[96];
    snprintf(line, sizeof(line), "Snapshot: %s", generated);
    textAt(20, 385, 2, line, 63);
    snprintf(line, sizeof(line), "Expires:  %s", expiry);
    textAt(20, 409, 2, line, 63);
    textAt(20, 446, 1, "Snapshot only; minutes do not count down. RT=realtime input, SCH=schedule estimate.", 125);
    textAt(20, 459, 1, "Screen persists without power. Check expiry; refresh is intentionally rate-limited.", 125);
  } while (display.nextPage());
  display.hibernate();
  last_screen = millis(); // Minimum interval begins after refresh finishes.
  screen_drawn = true;
}
#endif
} // namespace

void setup() {
  Serial.begin(115200);
#if ENABLE_LEDS
  leds.begin();
  leds.clear();
  leds.show(); // Boot into dark pixels before Wi-Fi or the e-paper driver starts.
  led_queue = xQueueCreate(1, sizeof(LedMessage));
  if (!led_queue || xTaskCreate(ledTask, "mrt-led-expiry", 8192, nullptr, 2, nullptr) != pdPASS) {
    Serial.println("LED SAFETY TASK FAILED; halted with pixels off");
    for (;;) delay(1000);
  }
#endif
#if ENABLE_EPAPER
  SPI.begin(EPAPER_SCK, -1, EPAPER_MOSI, EPAPER_CS);
  display.init(0, true, 2, false); // 2 ms reset for newer Waveshare adapter boards.
  display.setRotation(0);
#endif
  WiFi.mode(WIFI_STA);
  WiFi.setAutoReconnect(true);
  WiFi.begin(WIFI_SSID, WIFI_PASSWORD);
  configTime(0, 0, NTP_SERVER);
  last_poll = millis() - FRAME_POLL_MS;
}

void loop() {
  const uint32_t now = millis();
  if (WiFi.status() != WL_CONNECTED && mrt::elapsed(now, last_reconnect, kReconnectMs)) {
    last_reconnect = now;
    WiFi.reconnect();
  }
  if (have_frame && status == nullptr && !lease.alive(now)) fail("OFFLINE: SNAPSHOT EXPIRED");
  // Wall-clock jumps forward can invalidate the local lease early too.
  if (have_frame && status == nullptr && uint64_t(time(nullptr)) >= current.stamp.valid_until)
    fail("OFFLINE: SNAPSHOT EXPIRED");
  if (mrt::elapsed(now, last_poll, FRAME_POLL_MS)) {
    last_poll = now;
    attempted_poll = true;
    pollFrame();
  }
#if ENABLE_EPAPER
  renderScreen();
#endif
  delay(10);
}
