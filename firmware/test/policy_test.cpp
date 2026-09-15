#include "frame_policy.h"
#include <cassert>
#include <iostream>

int main() {
  constexpr uint64_t epoch = 1800000000;
  mrt::Lease lease;
  assert(lease.accept({epoch, epoch + 60}, epoch, 100) == mrt::Acceptance::New);
  assert(lease.alive(59099));
  assert(!lease.alive(59100));
  // Repeated HTTP responses must not renew the monotonic lease.
  assert(lease.accept({epoch, epoch + 60}, epoch + 15, 15100) == mrt::Acceptance::Duplicate);
  assert(lease.duration() == 59000);
  assert(lease.accept({epoch, epoch + 61}, epoch + 16, 16100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch - 1, epoch + 60}, epoch, 100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch + 6, epoch + 60}, epoch, 100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch + 1, epoch + 302}, epoch + 1, 100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch + 1, epoch + 60}, 0, 100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch + 1, epoch + 60}, epoch + 60, 100) == mrt::Acceptance::Rejected);
  lease.invalidate();
  assert(!lease.alive(100));
  assert(lease.accept({epoch, epoch + 60}, epoch, 100) == mrt::Acceptance::Rejected);
  assert(lease.accept({epoch + 1, epoch + 61}, epoch + 1, 100) == mrt::Acceptance::New);

  mrt::Lease rollover;
  assert(rollover.accept({epoch, epoch + 3}, epoch, UINT32_MAX - 999) == mrt::Acceptance::New);
  assert(rollover.alive(999));
  assert(!rollover.alive(1000));
  assert(mrt::elapsed(20, UINT32_MAX - 9, 30));

  mrt::Pixel pixels[] = {{0, 255, 255, 255}, {15, 255, 255, 255}};
  assert(mrt::validPixels(pixels, 2, 16));
  assert(mrt::validPixels(nullptr, 0, 16)); // sparse frame means omitted LEDs off.
  assert(!mrt::validPixels(pixels, 2, 15));
  assert(!mrt::validPixels(pixels, 2, 1));
  assert(!mrt::validPixels(pixels, 2, 513));
  pixels[1].index = 0;
  assert(!mrt::validPixels(pixels, 2, 16));
  pixels[1].index = 15;
  mrt::limitCurrent(pixels, 2, 16, 255, 46);
  uint32_t channels = 0;
  for (const auto& p : pixels) channels += p.r + p.g + p.b;
  assert(16 + channels * 20.0 / 255 <= 46);
  assert(pixels[0].r == pixels[1].r);
  mrt::limitCurrent(pixels, 2, 16, 0, 1000);
  assert(pixels[0].r == 0 && pixels[1].b == 0);
  pixels[0] = {0, 255, 0, 0};
  mrt::limitCurrent(pixels, 1, 16, 255, 10);
  assert(pixels[0].r == 0);

  mrt::Pixel network[146];
  for (uint16_t i = 0; i < 146; ++i) network[i] = {i, 255, 255, 255};
  assert(mrt::validPixels(network, 146, 146));
  mrt::limitCurrent(network, 146, 146, 32, 1000);
  channels = 0;
  for (const auto& p : network) channels += p.r + p.g + p.b;
  assert(channels > 0);
  assert(146 + channels * 20.0 / 255 <= 1000);
  std::cout << "policy tests passed\n";
}
