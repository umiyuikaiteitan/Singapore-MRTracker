#pragma once

#include <ArduinoJson.h>
#include <cstring>
#include "frame_policy.h"

namespace mrt {
constexpr size_t kMaxBodyBytes = 49152;
constexpr size_t kMaxRows = 8;
enum class Source { Realtime, Schedule, Stale, Unavailable };

struct Row {
  char line[13] = {};
  char destination[97] = {};
  uint32_t minutes = 0;
  bool approximate = false;
  bool realtime = false;
};

template <size_t PixelCount>
struct Frame {
  mrt::Stamp stamp;
  Source source = Source::Unavailable;
  mrt::Pixel pixels[PixelCount] = {};
  size_t pixel_count = 0;
  char station_name[81] = {};
  Row rows[kMaxRows];
  size_t row_count = 0;
  char notice[241] = {};
};


template <size_t N>
bool copyText(JsonVariantConst value, char (&target)[N], bool nonempty = false) {
  if (!value.is<const char*>()) return false;
  JsonString text = value.as<JsonString>();
  const size_t length = text.size();
  if (length >= N || (nonempty && length == 0) || strlen(text.c_str()) != length) return false;
  for (size_t i = 0; i < length; ++i) {
    const auto c = static_cast<unsigned char>(text.c_str()[i]);
    if (c < 0x20 || c == 0x7f) return false;
  }
  memcpy(target, text.c_str(), length);
  target[length] = '\0';
  return true;
}

template <size_t PixelCount>
const char* parseFrame(const char* json, size_t bytes, Frame<PixelCount>& frame,
                       const char* expected_layout) {
  static_assert(PixelCount > 0 && PixelCount <= kMaximumPixels, "Invalid pixel capacity");
  if (!json || bytes == 0 || bytes > kMaxBodyBytes) return "BODY SIZE LIMIT";
  DynamicJsonDocument document(8192 + JSON_ARRAY_SIZE(PixelCount) +
                               PixelCount * JSON_OBJECT_SIZE(4));
  const auto error = deserializeJson(document, json, bytes,
                                    DeserializationOption::NestingLimit(6));
  if (error || document.overflowed()) return "INVALID JSON OR BODY LIMIT";
  JsonObjectConst root = document.as<JsonObjectConst>();
  if (root.isNull() || !root["schema_version"].is<unsigned>() ||
      root["schema_version"].as<unsigned>() != 1) return "UNSUPPORTED FRAME SCHEMA";
  char layout[65];
  if (!copyText(root["layout_id"], layout, true) ||
      strcmp(layout, expected_layout) != 0) return "LAYOUT MISMATCH";
  if (!root["generated_at"].is<uint64_t>() || !root["valid_until"].is<uint64_t>())
    return "INVALID FRAME TIMESTAMPS";
  frame.stamp = {root["generated_at"].as<uint64_t>(), root["valid_until"].as<uint64_t>()};
  char source[16];
  if (!copyText(root["source_state"], source)) return "INVALID SOURCE STATE";
  if (strcmp(source, "realtime") == 0) frame.source = Source::Realtime;
  else if (strcmp(source, "schedule") == 0) frame.source = Source::Schedule;
  else if (strcmp(source, "stale") == 0) frame.source = Source::Stale;
  else if (strcmp(source, "unavailable") == 0) frame.source = Source::Unavailable;
  else return "UNKNOWN SOURCE STATE";

  if (!root["pixels"].is<JsonArrayConst>()) return "INVALID PIXEL ARRAY";
  JsonArrayConst pixels = root["pixels"].as<JsonArrayConst>();
  if (pixels.size() > PixelCount) return "TOO MANY PIXELS";
  frame.pixel_count = 0;
  for (JsonVariantConst item : pixels) {
    if (!item.is<JsonObjectConst>() || !item["index"].is<uint16_t>() ||
        !item["r"].is<uint8_t>() || !item["g"].is<uint8_t>() || !item["b"].is<uint8_t>())
      return "INVALID PIXEL VALUES";
    frame.pixels[frame.pixel_count++] = {item["index"].as<uint16_t>(),
      item["r"].as<uint8_t>(), item["g"].as<uint8_t>(), item["b"].as<uint8_t>()};
  }
  if (!mrt::validPixels(frame.pixels, frame.pixel_count, PixelCount))
    return "PIXEL INDEX OR DUPLICATE ERROR";

  if (!root["board"].is<JsonObjectConst>()) return "INVALID BOARD";
  JsonObjectConst board = root["board"].as<JsonObjectConst>();
  if (!copyText(board["station_name"], frame.station_name, true) ||
      !copyText(board["notice"], frame.notice) || !board["rows"].is<JsonArrayConst>())
    return "INVALID BOARD TEXT";
  JsonArrayConst rows = board["rows"].as<JsonArrayConst>();
  if (rows.size() > kMaxRows) return "TOO MANY BOARD ROWS";
  frame.row_count = 0;
  for (JsonVariantConst item : rows) {
    Row& row = frame.rows[frame.row_count++];
    if (!item.is<JsonObjectConst>() || !copyText(item["line"], row.line, true) ||
        !copyText(item["destination"], row.destination, true) ||
        !item["minutes"].is<uint32_t>() || !item["approximate"].is<bool>() ||
        !item["realtime"].is<bool>()) return "INVALID DEPARTURE ROW";
    row.minutes = item["minutes"].as<uint32_t>();
    row.approximate = item["approximate"].as<bool>();
    row.realtime = item["realtime"].as<bool>();
  }
  return nullptr;
}

} // namespace mrt
