#include "frame_decoder.h"
#include <cassert>
#include <iostream>
#include <string>

// Matches the Rust projection test's published schema-one fixture.
const std::string golden = R"({"schema_version":1,"layout_id":"fixture-mini-v1","generated_at":1000,"valid_until":1120,"source_state":"schedule","pixels":[{"index":0,"r":14,"g":8,"b":3},{"index":1,"r":14,"g":8,"b":3},{"index":2,"r":14,"g":8,"b":3},{"index":3,"r":14,"g":8,"b":3}],"board":{"station_name":"Woodlands North","rows":[{"line":"TEL","destination":"Springleaf","minutes":1,"approximate":false,"realtime":false},{"line":"TEL","destination":"Woodlands South","minutes":21,"approximate":false,"realtime":false}],"notice":"Timetable departures; live predictions unavailable. Station departure activity."}})";

std::string replace(std::string text, const std::string& from, const std::string& to) {
  const auto position = text.find(from);
  assert(position != std::string::npos);
  text.replace(position, from.size(), to);
  return text;
}

bool accepts(const std::string& json) {
  mrt::Frame<4> frame;
  return mrt::parseFrame(json.c_str(), json.size(), frame, "fixture-mini-v1") == nullptr;
}

int main() {
  mrt::Frame<4> frame;
  assert(mrt::parseFrame(golden.c_str(), golden.size(), frame, "fixture-mini-v1") == nullptr);
  assert(frame.pixel_count == 4 && frame.row_count == 2);
  assert(frame.source == mrt::Source::Schedule);
  assert(frame.stamp.generated_at == 1000 && frame.stamp.valid_until == 1120);
  assert(std::string(frame.rows[1].destination) == "Woodlands South");
  assert(frame.rows[1].minutes == 21 && !frame.rows[1].realtime);
  assert(accepts(replace(golden, "schedule", "stale")));
  assert(accepts(replace(golden, "schedule", "realtime")));
  assert(accepts(replace(golden, "schedule", "unavailable")));
  assert(!accepts(replace(golden, "schedule", "unknown")));
  assert(!accepts(replace(golden, "\"schema_version\":1", "\"schema_version\":2")));
  assert(!accepts(replace(golden, "fixture-mini-v1", "different-layout")));
  assert(!accepts(replace(golden, "\"generated_at\":1000", "\"generated_at\":-1")));
  assert(!accepts(replace(golden, "\"index\":1", "\"index\":0")));
  assert(!accepts(replace(golden, "\"index\":1", "\"index\":16")));
  assert(!accepts(replace(golden, "\"index\":1", "\"index\":1.5")));
  assert(!accepts(replace(golden, "\"r\":14", "\"r\":256")));
  assert(!accepts(replace(golden, "\"r\":14", "\"r\":-1")));
  assert(!accepts(replace(golden, "\"realtime\":false", "\"realtime\":\"false\"")));
  assert(!accepts(replace(golden, "\"minutes\":1", "\"minutes\":4294967296")));
  assert(!accepts(replace(golden, "Woodlands North", std::string(81, 's'))));
  assert(!accepts(replace(golden, "Springleaf", std::string(97, 'd'))));
  assert(!accepts(replace(golden, "TEL", std::string(13, 'l'))));
  assert(accepts(replace(golden, "TEL", std::string(12, 'l'))));
  assert(!accepts(replace(golden, "Woodlands North", "Woodlands\\u0000North")));
  assert(!accepts(replace(golden, "Woodlands North", "Woodlands\\nNorth")));
  assert(!accepts("{}"));
  assert(!accepts("[]"));
  assert(!accepts(golden.substr(0, golden.size() - 1)));
  assert(!accepts(std::string(mrt::kMaxBodyBytes + 1, ' ')));

  const auto rows_start = golden.find("\"rows\":[") + 8;
  const auto rows_end = golden.find("],\"notice\"", rows_start);
  const std::string row = R"({"line":"NSL","destination":"Jurong East","minutes":1,"approximate":true,"realtime":false})";
  std::string rows;
  for (size_t i = 0; i < 8; ++i) {
    if (i != 0) rows += ',';
    rows += row;
  }
  std::string eight_rows = golden;
  eight_rows.replace(rows_start, rows_end - rows_start, rows);
  assert(accepts(eight_rows));
  std::string nine_rows = golden;
  nine_rows.replace(rows_start, rows_end - rows_start, rows + ',' + row);
  assert(!accepts(nine_rows));

  // All 512 pixels must fit the decoder's actual ArduinoJson allocation, not
  // just pass a nominal limit in the lease/pixel policy.
  std::string pixels;
  for (size_t i = 0; i < 512; ++i) {
    if (i != 0) pixels += ',';
    pixels += "{\"index\":" + std::to_string(i) + ",\"r\":255,\"g\":255,\"b\":255}";
  }
  const auto start = golden.find("\"pixels\":[") + 10;
  const auto end = golden.find("],\"board\"", start);
  std::string maximum = golden;
  maximum.replace(start, end - start, pixels);
  mrt::Frame<512> large;
  assert(mrt::parseFrame(maximum.c_str(), maximum.size(), large, "fixture-mini-v1") == nullptr);
  assert(large.pixel_count == 512 && large.pixels[511].index == 511);
  assert(!accepts(maximum));
  std::string sparse = golden;
  sparse.replace(start, end - start, "");
  assert(accepts(sparse));
  assert(mrt::parseFrame(nullptr, 0, large, "fixture-mini-v1") != nullptr);
  std::cout << "parser tests passed\n";
}
