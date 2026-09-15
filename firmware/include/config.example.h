#pragma once

// Copy to config.h (gitignored). Never put the LTA/DataMall account key here.
#define WIFI_SSID "YOUR_WIFI_SSID"
#define WIFI_PASSWORD "YOUR_WIFI_PASSWORD"
// Trusted LAN only: unencrypted HTTP; use a VPN when crossing networks.
#define FRAME_URL "http://192.168.1.10:8787/v1/frame"
#define NTP_SERVER "pool.ntp.org" // A reachable local NTP server also works.
#define EXPECTED_LAYOUT_ID "singapore-mrt-v1" // Must exactly match service layout.
#define LED_COUNT 146
// LED_COUNT must match the physical layout, even on the separate e-paper device.
// Output type is selected by the led / epaper PlatformIO environment.
#define LED_PIN 13
#define LED_BRIGHTNESS 32 // 0..255, scaled before the current limiter.
#define LED_CURRENT_LIMIT_MA 1000 // LED budget only, excluding ESP32/display.
#define EPAPER_SCK 18
#define EPAPER_MOSI 23
#define EPAPER_CS 5
#define EPAPER_DC 17
#define EPAPER_RST 16
#define EPAPER_BUSY 4
#define FRAME_POLL_MS 15000UL
// Conservative full-refresh cadence; never configure below 60000 ms.
// Follow the documentation for the exact purchased panel's refresh limits.
#define EPAPER_REFRESH_MS 180000UL
