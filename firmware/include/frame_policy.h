#pragma once

#include <cstddef>
#include <cstdint>

namespace mrt {
constexpr uint32_t kMaxLeaseMs = 300000;
constexpr uint64_t kEarliestClock = 1700000000ULL;
constexpr uint64_t kFutureSkewSeconds = 5;
constexpr std::size_t kMaximumPixels = 512;

// Unsigned subtraction remains valid across millis() rollover for these short
// intervals. Callers tick often; they never compare absolute millis deadlines.
inline bool elapsed(uint32_t now, uint32_t started, uint32_t duration) {
  return uint32_t(now - started) >= duration;
}

struct Stamp {
  uint64_t generated_at = 0;
  uint64_t valid_until = 0;
};

enum class Acceptance { New, Duplicate, Rejected };

class Lease {
 public:
  Acceptance accept(Stamp stamp, uint64_t epoch, uint32_t now) {
    if (epoch < kEarliestClock || stamp.generated_at == 0 ||
        stamp.generated_at > epoch + kFutureSkewSeconds ||
        stamp.valid_until <= stamp.generated_at || stamp.valid_until <= epoch ||
        stamp.valid_until - stamp.generated_at > kMaxLeaseMs / 1000 ||
        stamp.valid_until - epoch > kMaxLeaseMs / 1000 ||
        stamp.generated_at < stamp_.generated_at) return Acceptance::Rejected;
    if (stamp.generated_at == stamp_.generated_at) {
      return stamp.valid_until == stamp_.valid_until && alive(now)
                 ? Acceptance::Duplicate : Acceptance::Rejected;
    }
    stamp_ = stamp;
    started_ = now;
    // time() has one-second resolution: subtract a second to avoid treating a
    // frame as valid beyond its server expiry due to rounding.
    const uint64_t remaining = stamp.valid_until - epoch;
    duration_ = remaining > 1 ? uint32_t((remaining - 1) * 1000) : 0;
    valid_ = duration_ != 0;
    return valid_ ? Acceptance::New : Acceptance::Rejected;
  }

  bool alive(uint32_t now) const {
    return valid_ && !elapsed(now, started_, duration_);
  }
  void invalidate() { valid_ = false; }
  uint32_t started() const { return started_; }
  uint32_t duration() const { return duration_; }
  Stamp stamp() const { return stamp_; }

 private:
  Stamp stamp_;
  uint32_t started_ = 0;
  uint32_t duration_ = 0;
  bool valid_ = false;
};

struct Pixel { uint16_t index; uint8_t r, g, b; };

inline bool validPixels(const Pixel* pixels, std::size_t count,
                        std::size_t configured_count) {
  if (configured_count == 0 || configured_count > kMaximumPixels ||
      count > configured_count) return false;
  bool seen[kMaximumPixels] = {};
  for (std::size_t i = 0; i < count; ++i) {
    if (pixels[i].index >= configured_count || seen[pixels[i].index]) return false;
    seen[pixels[i].index] = true;
  }
  return true;
}

// Worst-case estimate: 20 mA per full channel + 1 mA idle per physical pixel.
// Hardware fuse/wire/supply design remains necessary; this is an output cap.
inline void limitCurrent(Pixel* pixels, std::size_t count,
                         std::size_t physical_count, uint8_t brightness,
                         uint32_t budget_ma) {
  uint32_t channels = 0;
  for (std::size_t i = 0; i < count; ++i) {
    pixels[i].r = uint16_t(pixels[i].r) * brightness / 255;
    pixels[i].g = uint16_t(pixels[i].g) * brightness / 255;
    pixels[i].b = uint16_t(pixels[i].b) * brightness / 255;
    channels += pixels[i].r + pixels[i].g + pixels[i].b;
  }
  const uint32_t available = budget_ma > physical_count
      ? budget_ma - uint32_t(physical_count) : 0;
  const uint64_t channel_budget = uint64_t(available) * 255 / 20;
  if (channels <= channel_budget || channels == 0) return;
  for (std::size_t i = 0; i < count; ++i) {
    pixels[i].r = uint64_t(pixels[i].r) * channel_budget / channels;
    pixels[i].g = uint64_t(pixels[i].g) * channel_budget / channels;
    pixels[i].b = uint64_t(pixels[i].b) * channel_budget / channels;
  }
}
} // namespace mrt
