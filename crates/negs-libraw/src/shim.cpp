// A small C interface over LibRaw for Negs: open a file from memory, unpack
// the sensor data and hand back the visible CFA mosaic and the metadata the
// develop needs. Nothing else of LibRaw's processing (demosaic, colour,
// output) is used. Nikon HE decoding runs its tile loops on std::threads
// (LibRaw only threads them with OpenMP, which Apple's clang lacks).

#include "libraw/libraw.h"

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <thread>
#include <vector>

namespace {

template <typename F> int parallel_tiles(int num_tiles, F body) {
  unsigned threads = std::max(1u, std::min<unsigned>(std::thread::hardware_concurrency(), (unsigned)num_tiles));
  if (threads <= 1 || num_tiles <= 1) {
    for (int i = 0; i < num_tiles; ++i)
      if (int rv = body(i))
        return rv;
    return 0;
  }
  std::atomic<int> next{0};
  std::atomic<int> error{0};
  std::vector<std::thread> pool;
  pool.reserve(threads);
  for (unsigned t = 0; t < threads; ++t)
    pool.emplace_back([&] {
      for (int i = next++; i < num_tiles && error.load() == 0; i = next++)
        if (int rv = body(i)) {
          int expected = 0;
          error.compare_exchange_strong(expected, rv);
        }
    });
  for (auto &th : pool)
    th.join();
  return error.load();
}

class NegsLibRaw : public LibRaw {
public:
  NegsLibRaw() : LibRaw(LIBRAW_OPIONS_NO_DATAERR_CALLBACK) {}

protected:
  int nikonhe_decode_loop1(int num_tiles, int tile_height, const void *tile_array, void *dec_ptr) override {
    return parallel_tiles(num_tiles, [&](int i) { return nikonhe_decode_loop1_body(i, num_tiles, tile_height, tile_array, dec_ptr); });
  }
  int nikonhe_decode_loop2(int num_tiles, int tile_height, void *dec_ptr) override {
    parallel_tiles(num_tiles, [&](int i) {
      nikonhe_decode_loop2_body(i, num_tiles, tile_height, dec_ptr);
      return 0;
    });
    return 0;
  }
  int nikonhe_decode_loop3(int num_tiles, int tile_height, void *dec_ptr) override {
    return parallel_tiles(num_tiles, [&](int i) { return nikonhe_decode_loop3_body(i, num_tiles, tile_height, dec_ptr); });
  }
};

} // namespace

extern "C" {

struct negs_libraw_info {
  uint32_t width, height;     // the visible area
  uint32_t raw_width, raw_height, top_margin, left_margin; // the whole sensor, and where the visible area lies on it
  uint32_t cfa_width, cfa_height; // 2x2 Bayer, 6x6 X-Trans
  uint8_t cfa[36];            // colour index (0 R, 1 G, 2 B, 3 second green / E) per cell
  uint32_t colors;
  float black[36];            // black level per CFA cell
  uint32_t white;
  float cam_mul[4];           // as-shot white balance multipliers
  float cam_xyz[4][3];        // XYZ (D65) to camera
  char make[64];
  char model[64];
  int32_t flip;               // LibRaw's flip: 0, 3 (180), 5 (90 CCW), 6 (90 CW)
  uint32_t crop_left, crop_top, crop_width, crop_height; // recommended crop in the visible area, 0 = none
  char decoder[64];
};

void *negs_libraw_new(void) {
  try {
    return new NegsLibRaw();
  } catch (...) {
    return nullptr;
  }
}

void negs_libraw_free(void *handle) { delete static_cast<NegsLibRaw *>(handle); }

const char *negs_libraw_strerror(int code) { return libraw_strerror(code); }

const char *negs_libraw_version(void) { return LibRaw::version(); }

// Opens and unpacks `data` (which must outlive the handle) and fills `info`.
// Returns a LibRaw error code (0 on success) or -1000 for a layout Negs
// doesn't take (no CFA mosaic).
int negs_libraw_unpack(void *handle, const void *data, size_t size, negs_libraw_info *info) {
  auto *lr = static_cast<NegsLibRaw *>(handle);
  try {
    int rv = lr->open_buffer(data, size);
    if (rv != LIBRAW_SUCCESS)
      return rv;
    rv = lr->unpack();
    if (rv != LIBRAW_SUCCESS)
      return rv;
    auto &d = lr->imgdata;
    if (!d.rawdata.raw_image || d.idata.filters == 0)
      return -1000;
    std::memset(info, 0, sizeof *info);
    info->width = d.sizes.width;
    info->height = d.sizes.height;
    info->raw_width = d.sizes.raw_width;
    info->raw_height = d.sizes.raw_height;
    info->top_margin = d.sizes.top_margin;
    info->left_margin = d.sizes.left_margin;
    bool xtrans = d.idata.filters == 9;
    info->cfa_width = info->cfa_height = xtrans ? 6 : 2;
    if (!xtrans && d.idata.filters < 1000)
      return -1000; // odd dcraw patterns (Leaf, some old backs)
    info->colors = d.idata.colors;
    const unsigned *cb = d.color.cblack;
    for (unsigned r = 0; r < info->cfa_height; ++r)
      for (unsigned c = 0; c < info->cfa_width; ++c) {
        int color = lr->COLOR(r, c);
        unsigned i = r * info->cfa_width + c;
        info->cfa[i] = (uint8_t)color;
        float black = (float)d.color.black + (float)cb[color & 3];
        // Before LibRaw's adjust_bl: the pattern is in visible coordinates
        // (as subtract_black indexes it).
        if (cb[4] && cb[5])
          black += (float)cb[6 + (r % cb[4]) * cb[5] + c % cb[5]];
        info->black[i] = black;
      }
    info->white = d.color.maximum;
    for (int i = 0; i < 4; ++i)
      info->cam_mul[i] = d.color.cam_mul[i];
    for (int i = 0; i < 4; ++i)
      for (int j = 0; j < 3; ++j)
        info->cam_xyz[i][j] = d.color.cam_xyz[i][j];
    std::strncpy(info->make, d.idata.make, sizeof info->make - 1);
    std::strncpy(info->model, d.idata.model, sizeof info->model - 1);
    info->flip = d.sizes.flip;
    const auto &crop = d.sizes.raw_inset_crops[0];
    if (crop.cwidth && crop.cheight && crop.cleft != 0xffff && crop.ctop != 0xffff) {
      // raw_inset_crops is in raw coordinates.
      long left = (long)crop.cleft - d.sizes.left_margin, top = (long)crop.ctop - d.sizes.top_margin;
      if (left >= 0 && top >= 0 && left + crop.cwidth <= info->width && top + crop.cheight <= info->height) {
        info->crop_left = (uint32_t)left;
        info->crop_top = (uint32_t)top;
        info->crop_width = crop.cwidth;
        info->crop_height = crop.cheight;
      }
    }
    libraw_decoder_info_t di;
    if (lr->get_decoder_info(&di) == LIBRAW_SUCCESS && di.decoder_name)
      std::strncpy(info->decoder, di.decoder_name, sizeof info->decoder - 1);
    return LIBRAW_SUCCESS;
  } catch (...) {
    return LIBRAW_UNSPECIFIED_ERROR;
  }
}

// Copies the visible mosaic (width × height) into `out`, or with `whole` the
// whole sensor (raw_width × raw_height).
int negs_libraw_copy(void *handle, uint16_t *out, int whole) {
  auto *lr = static_cast<NegsLibRaw *>(handle);
  auto &d = lr->imgdata;
  if (!d.rawdata.raw_image)
    return -1000;
  size_t pitch = d.sizes.raw_pitch / 2;
  unsigned top = whole ? 0 : d.sizes.top_margin, left = whole ? 0 : d.sizes.left_margin;
  unsigned width = whole ? d.sizes.raw_width : d.sizes.width, height = whole ? d.sizes.raw_height : d.sizes.height;
  for (unsigned r = 0; r < height; ++r)
    std::memcpy(out + (size_t)r * width, d.rawdata.raw_image + (r + top) * pitch + left, width * sizeof(uint16_t));
  return 0;
}

} // extern "C"
